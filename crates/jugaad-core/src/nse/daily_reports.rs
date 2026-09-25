//! Downloads any of NSE's 39+ daily report types by a `fileKey`, discovered
//! through a metadata API, instead of needing a hand-built URL for each one
//! the way the specific fetchers in `archives.rs` do. Only covers the
//! current and previous trading day - confirmed live, and matching the
//! Python original's own docstring: NSE doesn't expose historical data
//! through this API, only `full_bhavcopy_raw`/etc. go further back.
//!
//! Unlike `NseHistory`, this needs no cookie warm-up - confirmed by testing
//! the metadata endpoint from a completely fresh session.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use reqwest::StatusCode;
use serde::Deserialize;

use super::dates::deserialize_nse_date;
use super::http::{HttpClient, client_builder, with_retry};
use crate::error::{Error, Result};

const METADATA_URL: &str = "https://www.nseindia.com/api/daily-reports";

/// One available file for a report type on a specific trading day, as NSE's
/// metadata API describes it.
#[derive(Debug, Clone, Deserialize)]
struct ReportFile {
    #[serde(rename = "fileKey")]
    file_key: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "fileActlName")]
    file_name: String,
    #[serde(rename = "filePath")]
    file_path: String,
    #[serde(rename = "tradingDate", deserialize_with = "deserialize_nse_date")]
    trading_date: NaiveDate,
    #[serde(rename = "fileSize")]
    file_size: String,
}

// An unknown `segment` comes back as `{"data":[],"msg":"no data found"}`
// instead of the normal shape below - `#[serde(default)]` means neither
// field is present, so it just looks like "no files today or yesterday
// either," which is the same ambiguous-empty-result pattern used
// throughout this crate. `FutureDay` also exists in the real response but
// is unused here, matching the Python original's own `find_file`/
// `list_available_files`, which never look at it either.
#[derive(Debug, Default, Deserialize)]
struct DailyReportsResponse {
    #[serde(rename = "CurrentDay", default)]
    current_day: Vec<ReportFile>,
    #[serde(rename = "PreviousDay", default)]
    previous_day: Vec<ReportFile>,
}

/// One report type available for a segment, with every dated file NSE is
/// currently offering for it (usually today's and/or yesterday's).
#[derive(Debug)]
pub struct ReportSummary {
    pub file_key: String,
    pub display_name: String,
    pub dates: Vec<ReportDate>,
}

/// One dated file within a `ReportSummary`.
#[derive(Debug)]
pub struct ReportDate {
    pub trading_date: NaiveDate,
    pub file_size: String,
    pub file_name: String,
}

#[derive(Debug, Clone)]
pub struct NseDailyReports {
    client: HttpClient,
}

impl NseDailyReports {
    pub fn new() -> Result<Self> {
        let client = client_builder().build()?;
        Ok(Self {
            client: with_retry(client),
        })
    }

    /// Lists every report type NSE currently offers for `segment` (e.g.
    /// `"CM"` for capital market, `"FO"` for derivatives), each with the
    /// current/previous-day files available for it.
    pub async fn list_available_reports(&self, segment: &str) -> Result<Vec<ReportSummary>> {
        let files = self.fetch_reports(segment).await?;
        Ok(group_by_file_key(files))
    }

    /// Downloads one report by its file key (e.g. `"CM-BULK-DEAL"`,
    /// `"CM-VOLATILITY"` - see `list_available_reports` for what's
    /// available for a segment). Prefers today's file, falling back to
    /// yesterday's if today's isn't listed. Returns raw bytes rather than
    /// text: report formats vary (CSV, zip, proprietary `.DAT` files, ...),
    /// unlike everything else in this crate there's no single expected
    /// content type here.
    pub async fn download_report_raw(&self, file_key: &str, segment: &str) -> Result<Vec<u8>> {
        let file = self.find_file(file_key, segment).await?;
        self.fetch_file_bytes(&file).await
    }

    /// Downloads the report the same way as `download_report_raw`, then
    /// writes it into `dest` (a directory) under NSE's own filename for it.
    /// Unlike every other `_save` method in this crate, there's no date or
    /// range to build a filename from ourselves, so NSE's name is used
    /// directly. Returns the path written.
    pub async fn download_report_save(
        &self,
        file_key: &str,
        segment: &str,
        dest: &Path,
    ) -> Result<PathBuf> {
        let file = self.find_file(file_key, segment).await?;
        let bytes = self.fetch_file_bytes(&file).await?;

        let path = dest.join(&file.file_name);
        std::fs::write(&path, &bytes)?;
        Ok(path)
    }

    /// Finds the metadata entry for `file_key`, preferring the current
    /// day's file over the previous day's if both are listed - matches the
    /// Python original's search order exactly.
    async fn find_file(&self, file_key: &str, segment: &str) -> Result<ReportFile> {
        let files = self.fetch_reports(segment).await?;
        files
            .into_iter()
            .find(|file| file.file_key == file_key)
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "no file key '{file_key}' in segment '{segment}' (see list_available_reports)"
                ))
            })
    }

    async fn fetch_file_bytes(&self, file: &ReportFile) -> Result<Vec<u8>> {
        let url = format!("{}{}", file.file_path, file.file_name);
        let response = self.client.get(url).send().await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Err(Error::NoData),
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        Ok(response.bytes().await?.to_vec())
    }

    async fn fetch_reports(&self, segment: &str) -> Result<Vec<ReportFile>> {
        let response = self
            .client
            .get(METADATA_URL)
            .query(&[("key", segment)])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: DailyReportsResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse daily reports response: {e}")))?;

        // Current day first, so `find_file`'s `.find()` naturally prefers it.
        let mut files = parsed.current_day;
        files.extend(parsed.previous_day);
        Ok(files)
    }
}

/// Groups the flat file list into one `ReportSummary` per distinct
/// `file_key`, preserving the order file keys first appeared in.
fn group_by_file_key(files: Vec<ReportFile>) -> Vec<ReportSummary> {
    let mut summaries: Vec<ReportSummary> = Vec::new();
    for file in files {
        let dated = ReportDate {
            trading_date: file.trading_date,
            file_size: file.file_size,
            file_name: file.file_name,
        };
        match summaries
            .iter_mut()
            .find(|summary| summary.file_key == file.file_key)
        {
            Some(summary) => summary.dates.push(dated),
            None => summaries.push(ReportSummary {
                file_key: file.file_key,
                display_name: file.display_name,
                dates: vec![dated],
            }),
        }
    }
    summaries
}

// Panicking via `.unwrap()` on a failed assertion is the normal, intended
// way for a test to fail - the workspace-wide `unwrap_used` lint is aimed at
// production code paths, not test code, hence the blanket allow below.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    // Real response shape captured from NSE's daily-reports API for CM.
    const SAMPLE_RESPONSE: &str =
        include_str!("../../tests/fixtures/daily_reports/sample_response.json");

    #[test]
    fn deserializes_real_response_shape() {
        let parsed: DailyReportsResponse = serde_json::from_str(SAMPLE_RESPONSE).unwrap();

        assert_eq!(parsed.current_day.len(), 1);
        assert_eq!(parsed.previous_day.len(), 1);
        assert_eq!(parsed.current_day[0].file_key, "CM-UDIFF-BHAVCOPY-CSV");
        assert_eq!(parsed.current_day[0].trading_date, date(2026, 9, 18));
    }

    // An unknown `segment` gets this shape instead - confirmed live.
    const SAMPLE_INVALID_SEGMENT_RESPONSE: &str =
        include_str!("../../tests/fixtures/daily_reports/sample_invalid_segment_response.json");

    #[test]
    fn invalid_segment_shape_deserializes_to_empty_lists() {
        let parsed: DailyReportsResponse =
            serde_json::from_str(SAMPLE_INVALID_SEGMENT_RESPONSE).unwrap();

        assert!(parsed.current_day.is_empty());
        assert!(parsed.previous_day.is_empty());
    }

    #[test]
    fn group_by_file_key_combines_same_key_across_days() {
        let parsed: DailyReportsResponse = serde_json::from_str(SAMPLE_RESPONSE).unwrap();
        let mut files = parsed.current_day;
        files.extend(parsed.previous_day);

        let summaries = group_by_file_key(files);

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].file_key, "CM-UDIFF-BHAVCOPY-CSV");
        assert_eq!(summaries[0].dates.len(), 2);
        // Current day's entry was concatenated first, so it comes first here.
        assert_eq!(summaries[0].dates[0].trading_date, date(2026, 9, 18));
        assert_eq!(summaries[0].dates[1].trading_date, date(2026, 9, 17));
    }
}
