use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use reqwest::{Client, StatusCode};
use zip::ZipArchive;

use super::USER_AGENT;
use crate::error::{Error, Result};

const BASE_URL: &str = "https://nsearchives.nseindia.com";
const REPORTS_URL: &str = "https://www.nseindia.com/api/reports";

// Fixed value for the old bhavcopy endpoint's `archives` query param -
// identifies which report to fetch. Not date-dependent.
const OLD_BHAVCOPY_ARCHIVES: &str = r#"[{"name": "CM - Bhavcopy(csv)", "type": "daily-reports", "category": "capital-market", "section": "equities"}]"#;

// NSE switched bhavcopy to the "UDiff" format on this date - see
// docs/nse-findings.md. Dates before it need the old endpoint/format.
fn udiff_start_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 7, 8).unwrap_or(NaiveDate::MIN)
}

#[derive(Debug)]
pub struct NseArchives {
    client: Client,
}

impl NseArchives {
    pub fn new() -> Result<Self> {
        let client = Client::builder().user_agent(USER_AGENT).build()?;
        Ok(Self { client })
    }

    /// Fetches the bhavcopy CSV text for a single trading day, automatically
    /// using NSE's current "UDiff" format for `dt >= 2024-07-08` and the
    /// older format (different columns, but includes ISIN) for earlier dates.
    pub async fn bhavcopy_raw(&self, dt: NaiveDate) -> Result<String> {
        if dt < udiff_start_date() {
            self.bhavcopy_old_raw(dt).await
        } else {
            self.bhavcopy_udiff_raw(dt).await
        }
    }

    async fn bhavcopy_udiff_raw(&self, dt: NaiveDate) -> Result<String> {
        let url = format!(
            "{BASE_URL}/content/cm/BhavCopy_NSE_CM_0_0_0_{}_F_0000.csv.zip",
            dt.format("%Y%m%d")
        );
        fetch_zip_csv(self.client.get(url)).await
    }

    // Verified live: unlike Python's version, this doesn't need a
    // cookie-warm-up-then-retry dance today - a fresh session with no prior
    // cookies gets a normal 200. If NSE starts requiring one again, it'll
    // surface as `Error::Blocked` via `fetch_zip_csv`'s status handling.
    async fn bhavcopy_old_raw(&self, dt: NaiveDate) -> Result<String> {
        let date_str = dt.format("%d-%b-%Y").to_string();

        let request = self
            .client
            .get(REPORTS_URL)
            .query(&[
                ("archives", OLD_BHAVCOPY_ARCHIVES),
                ("date", date_str.as_str()),
                ("type", "equities"),
                ("mode", "single"),
            ])
            .header("Referer", "https://www.nseindia.com/all-reports");
        fetch_zip_csv(request).await
    }

    pub async fn bhavcopy_save(&self, dt: NaiveDate, dest: &Path) -> Result<PathBuf> {
        let file_name = format!("cm{}bhav.csv", dt.format("%d%b%Y"));
        let path = dest.join(file_name);

        if path.is_file() {
            return Ok(path);
        }

        let text = self.bhavcopy_raw(dt).await?;
        std::fs::write(&path, text)?;
        Ok(path)
    }

    /// Fetches the F&O (derivatives) bhavcopy CSV text for a single trading
    /// day, automatically picking the right format the same way
    /// `bhavcopy_raw` does for equities (NSE switched both on the same date).
    pub async fn bhavcopy_fo_raw(&self, dt: NaiveDate) -> Result<String> {
        if dt < udiff_start_date() {
            self.bhavcopy_fo_old_raw(dt).await
        } else {
            self.bhavcopy_fo_udiff_raw(dt).await
        }
    }

    async fn bhavcopy_fo_udiff_raw(&self, dt: NaiveDate) -> Result<String> {
        let url = format!(
            "{BASE_URL}/content/fo/BhavCopy_NSE_FO_0_0_0_{}_F_0000.csv.zip",
            dt.format("%Y%m%d")
        );
        fetch_zip_csv(self.client.get(url)).await
    }

    async fn bhavcopy_fo_old_raw(&self, dt: NaiveDate) -> Result<String> {
        let year = dt.format("%Y");
        let day = dt.format("%d");
        let month_upper = dt.format("%b").to_string().to_uppercase();
        let url = format!(
            "{BASE_URL}/content/historical/DERIVATIVES/{year}/{month_upper}/fo{day}{month_upper}{year}bhav.csv.zip"
        );
        fetch_zip_csv(self.client.get(url)).await
    }

    pub async fn bhavcopy_fo_save(&self, dt: NaiveDate, dest: &Path) -> Result<PathBuf> {
        let file_name = format!("fo{}bhav.csv", dt.format("%d%b%Y"));
        let path = dest.join(file_name);

        if path.is_file() {
            return Ok(path);
        }

        let text = self.bhavcopy_fo_raw(dt).await?;
        std::fs::write(&path, text)?;
        Ok(path)
    }

    /// Fetches the "full" bhavcopy CSV text for a single trading day - like
    /// `bhavcopy_raw`, but every series (not just EQ) and with delivery
    /// quantity/percentage columns. Plain CSV, not zipped. No format
    /// migration here - one shape covers every date this endpoint serves.
    pub async fn full_bhavcopy_raw(&self, dt: NaiveDate) -> Result<String> {
        let url = format!(
            "{BASE_URL}/products/content/sec_bhavdata_full_{}.csv",
            dt.format("%d%m%Y")
        );

        let response = self.client.get(url).send().await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Err(Error::NoData),
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        Ok(response.text().await?)
    }

    pub async fn full_bhavcopy_save(&self, dt: NaiveDate, dest: &Path) -> Result<PathBuf> {
        let file_name = format!("sec_bhavdata_full_{}.csv", dt.format("%d%b%Y"));
        let path = dest.join(file_name);

        if path.is_file() {
            return Ok(path);
        }

        let text = self.full_bhavcopy_raw(dt).await?;
        std::fs::write(&path, text)?;
        Ok(path)
    }

    /// Fetches the current bulk deals report as CSV text. Unlike every other
    /// fetcher in this module, this isn't parameterized by date - NSE only
    /// serves the latest snapshot at this URL, not a queryable history.
    pub async fn bulk_deals_raw(&self) -> Result<String> {
        let response = self
            .client
            .get(format!("{BASE_URL}/content/equities/bulk.csv"))
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Err(Error::NoData),
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        Ok(response.text().await?)
    }

    /// Fetches the current bulk deals report and writes it to `path`
    /// exactly - unlike the other `_save` methods, this takes a full file
    /// path rather than a destination directory, since there's no date to
    /// derive a filename from, and always overwrites rather than skipping
    /// if present, since the data is a live snapshot that changes intraday.
    pub async fn bulk_deals_save(&self, path: &Path) -> Result<PathBuf> {
        let text = self.bulk_deals_raw().await?;
        std::fs::write(path, text)?;
        Ok(path.to_path_buf())
    }
}

/// Sends `request`, checks for the status codes shared by every bhavcopy
/// endpoint in this module, then unzips the single CSV file in the response.
async fn fetch_zip_csv(request: reqwest::RequestBuilder) -> Result<String> {
    let response = request.send().await?;

    match response.status() {
        StatusCode::OK => {}
        StatusCode::NOT_FOUND => return Err(Error::NoData),
        StatusCode::FORBIDDEN => return Err(Error::Blocked),
        status => return Err(Error::UnexpectedStatus(status)),
    }

    let bytes = response.bytes().await?;
    unzip_single_csv(&bytes)
}

fn unzip_single_csv(data: &[u8]) -> Result<String> {
    let reader = Cursor::new(data);
    let mut archive =
        ZipArchive::new(reader).map_err(|e| Error::Parse(format!("bad zip archive: {e}")))?;

    let mut entry = archive
        .by_index(0)
        .map_err(|e| Error::Parse(format!("could not read zip entry: {e}")))?;

    let mut contents = String::new();
    entry
        .read_to_string(&mut contents)
        .map_err(|e| Error::Parse(format!("zip entry was not valid utf-8: {e}")))?;

    Ok(contents)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;

    fn make_test_zip(filename: &str, contents: &str) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(filename, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents.as_bytes()).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn unzip_single_csv_extracts_the_one_entry() {
        let zip_bytes = make_test_zip("data.csv", "a,b,c\n1,2,3\n");
        let text = unzip_single_csv(&zip_bytes).unwrap();
        assert_eq!(text, "a,b,c\n1,2,3\n");
    }

    #[test]
    fn unzip_single_csv_rejects_non_zip_data() {
        let err = unzip_single_csv(b"not a zip file").unwrap_err();
        assert!(matches!(err, Error::Parse(_)));
    }

    #[test]
    fn udiff_start_date_is_2024_07_08() {
        assert_eq!(
            udiff_start_date(),
            NaiveDate::from_ymd_opt(2024, 7, 8).unwrap()
        );
    }
}
