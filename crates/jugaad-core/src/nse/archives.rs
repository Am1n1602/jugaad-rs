use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use reqwest::{Client, StatusCode};
use zip::ZipArchive;

use super::USER_AGENT;
use crate::error::{Error, Result};

const BASE_URL: &str = "https://nsearchives.nseindia.com";

#[derive(Debug)]
pub struct NseArchives {
    client: Client,
}

impl NseArchives {
    pub fn new() -> Result<Self> {
        let client = Client::builder().user_agent(USER_AGENT).build()?;
        Ok(Self { client })
    }

    /// Fetches the bhavcopy CSV text for a single trading day.
    pub async fn bhavcopy_raw(&self, dt: NaiveDate) -> Result<String> {
        let url = format!(
            "{BASE_URL}/content/cm/BhavCopy_NSE_CM_0_0_0_{}_F_0000.csv.zip",
            dt.format("%Y%m%d")
        );

        let response = self.client.get(&url).send().await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Err(Error::NoData),
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let bytes = response.bytes().await?;
        unzip_single_csv(&bytes)
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
}
