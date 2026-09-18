//! Downloads historical OHLC data for NSE indices (e.g. "NIFTY 50") from
//! niftyindices.com - a different site from the rest of this module, with
//! its own set of quirks.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::NaiveDate;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Deserializer, Serialize};
use tokio::sync::Semaphore;

use super::USER_AGENT;
use super::dates::break_into_month_chunks;
use crate::error::{Error, Result};

const BASE_URL: &str = "https://niftyindices.com";
const MAX_CONCURRENT_REQUESTS: usize = 2;

/// One trading day's OHLC data for an index.
///
/// Like `StockHistoryRow`, `rename(deserialize = "...")` keeps NSE's raw
/// field names to reading the API response only - writing this out (e.g.
/// to CSV) uses the clean field names below.
#[derive(Debug, Serialize, Deserialize)]
pub struct IndexHistoryRow {
    #[serde(rename(deserialize = "INDEX_NAME"))]
    pub index_name: String,
    #[serde(
        rename(deserialize = "HistoricalDate"),
        deserialize_with = "deserialize_index_date"
    )]
    pub date: NaiveDate,
    #[serde(
        rename(deserialize = "OPEN"),
        deserialize_with = "deserialize_string_f64"
    )]
    pub open: f64,
    #[serde(
        rename(deserialize = "HIGH"),
        deserialize_with = "deserialize_string_f64"
    )]
    pub high: f64,
    #[serde(
        rename(deserialize = "LOW"),
        deserialize_with = "deserialize_string_f64"
    )]
    pub low: f64,
    #[serde(
        rename(deserialize = "CLOSE"),
        deserialize_with = "deserialize_string_f64"
    )]
    pub close: f64,
}

// This endpoint's dates look like "05 Aug 2024" (space-separated) - a
// different format than the stock history API's "05-Aug-2024" (hyphenated).
fn deserialize_index_date<'de, D>(deserializer: D) -> std::result::Result<NaiveDate, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    NaiveDate::parse_from_str(&raw, "%d %b %Y").map_err(serde::de::Error::custom)
}

// niftyindices sends numbers as JSON strings, e.g. "OPEN":"24302.85"
// instead of "OPEN":24302.85.
fn deserialize_string_f64<'de, D>(deserializer: D) -> std::result::Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.parse().map_err(serde::de::Error::custom)
}

/// Builds the request body niftyindices' API actually expects: a JSON object
/// whose `cinfo` field is a *string* that looks like a single-quoted Python
/// dict literal, not nested JSON. Confirmed against the live API - sending
/// proper `{"cinfo": {...}}` gets rejected. Only safe because index names
/// never contain a quote or brace character.
fn build_request_body(name: &str, from_date: NaiveDate, to_date: NaiveDate) -> String {
    let from_str = from_date.format("%d-%b-%Y");
    let to_str = to_date.format("%d-%b-%Y");
    let cinfo = format!(
        "{{'name': '{name}', 'startDate': '{from_str}', 'endDate': '{to_str}', 'indexName': '{name}'}}"
    );
    serde_json::json!({ "cinfo": cinfo }).to_string()
}

#[derive(Debug, Clone)]
pub struct NseIndexHistory {
    client: Client,
}

impl NseIndexHistory {
    pub fn new() -> Result<Self> {
        let client = Client::builder().user_agent(USER_AGENT).build()?;
        Ok(Self { client })
    }

    pub async fn index_history_raw(
        &self,
        name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<IndexHistoryRow>> {
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

        let mut tasks = Vec::new();
        for (chunk_start, chunk_end) in break_into_month_chunks(from_date, to_date) {
            let history = self.clone();
            let name = name.to_string();
            let semaphore = Arc::clone(&semaphore);

            tasks.push(tokio::spawn(async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .map_err(|e| Error::Parse(format!("semaphore closed unexpectedly: {e}")))?;
                history.fetch_chunk(&name, chunk_start, chunk_end).await
            }));
        }

        let mut rows = Vec::new();
        for task in tasks {
            rows.extend(task.await??);
        }

        Ok(rows)
    }

    /// Fetches history the same way as `index_history_raw`, then writes it
    /// as a CSV file into `dest` (a directory - the filename is derived from
    /// the index name and date range). Returns the path written.
    pub async fn index_history_csv(
        &self,
        name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self.index_history_raw(name, from_date, to_date).await?;

        let file_name = format!("{name}-{from_date}-{to_date}.csv");
        let path = dest.join(file_name);

        let mut writer = csv::Writer::from_path(&path)?;
        for row in &rows {
            writer.serialize(row)?;
        }
        writer.flush()?;

        Ok(path)
    }

    /// Fetches one chunk directly from niftyindices.com in a single request.
    /// Callers should use `index_history_raw`, which chunks long ranges.
    async fn fetch_chunk(
        &self,
        name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<IndexHistoryRow>> {
        let body = build_request_body(name, from_date, to_date);

        let response = self
            .client
            .post(format!(
                "{BASE_URL}/BackPage/getHistoricaldatatabletoString"
            ))
            .header("Content-Type", "application/json; charset=UTF-8")
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Referer", BASE_URL)
            .header("Origin", BASE_URL)
            .body(body)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            status => return Err(Error::UnexpectedStatus(status)),
        }

        response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse index history response: {e}")))
    }
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

    #[test]
    fn builds_the_quirky_cinfo_body_nse_actually_expects() {
        let body = build_request_body("NIFTY 50", date(2024, 8, 1), date(2024, 8, 5));
        assert_eq!(
            body,
            r#"{"cinfo":"{'name': 'NIFTY 50', 'startDate': '01-Aug-2024', 'endDate': '05-Aug-2024', 'indexName': 'NIFTY 50'}"}"#
        );
    }

    // Real response captured from niftyindices for NIFTY 50.
    const SAMPLE_RESPONSE: &str = r#"[{"RequestNumber":"His639","Index Name":"",
        "INDEX_NAME":"Nifty 50","HistoricalDate":"05 Aug 2024","OPEN":"24302.85",
        "HIGH":"24350.05","LOW":"23893.7","CLOSE":"24055.60"}]"#;

    #[test]
    fn deserializes_real_response_shape() {
        let rows: Vec<IndexHistoryRow> = serde_json::from_str(SAMPLE_RESPONSE).unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].index_name, "Nifty 50");
        assert_eq!(rows[0].date, date(2024, 8, 5));
        assert_eq!(rows[0].open, 24302.85);
        assert_eq!(rows[0].close, 24055.60);
    }

    #[test]
    fn serializing_uses_clean_field_names_not_nse_names() {
        let rows: Vec<IndexHistoryRow> = serde_json::from_str(SAMPLE_RESPONSE).unwrap();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&rows[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(header, "index_name,date,open,high,low,close");
    }

    #[test]
    fn empty_array_response_deserializes_to_empty_vec() {
        let rows: Vec<IndexHistoryRow> = serde_json::from_str("[]").unwrap();
        assert!(rows.is_empty());
    }
}
