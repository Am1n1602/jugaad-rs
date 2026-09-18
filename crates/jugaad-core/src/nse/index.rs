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

/// One day's P/E, P/B and dividend yield for an index.
#[derive(Debug, Serialize, Deserialize)]
pub struct IndexPeRow {
    #[serde(rename(deserialize = "Index Name"))]
    pub index_name: String,
    #[serde(
        rename(deserialize = "DATE"),
        deserialize_with = "deserialize_index_date"
    )]
    pub date: NaiveDate,
    #[serde(
        rename(deserialize = "pe"),
        deserialize_with = "deserialize_string_f64"
    )]
    pub pe: f64,
    #[serde(
        rename(deserialize = "pb"),
        deserialize_with = "deserialize_string_f64"
    )]
    pub pb: f64,
    #[serde(
        rename(deserialize = "divYield"),
        deserialize_with = "deserialize_string_f64"
    )]
    pub div_yield: f64,
}

/// One day's Total Return Index value for an index.
#[derive(Debug, Serialize, Deserialize)]
pub struct IndexTriRow {
    #[serde(rename(deserialize = "Index Name"))]
    pub index_name: String,
    #[serde(
        rename(deserialize = "Date"),
        deserialize_with = "deserialize_index_date"
    )]
    pub date: NaiveDate,
    #[serde(
        rename(deserialize = "TotalReturnsIndex"),
        deserialize_with = "deserialize_string_f64"
    )]
    pub total_returns_index: f64,
    #[serde(
        rename(deserialize = "NTR_Value"),
        deserialize_with = "deserialize_string_f64"
    )]
    pub ntr_value: f64,
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

/// Builds the request body niftyindices' history-style endpoints expect: a
/// JSON object whose `cinfo` field is a *string* that looks like a
/// single-quoted Python dict literal, not nested JSON. Confirmed against
/// the live API - sending proper `{"cinfo": {...}}` gets rejected. Only
/// safe because index names never contain a quote or brace character.
///
/// `name` and `index_name` are usually the same value (that's what every
/// caller in this file except `index_tri_history_raw` passes), but the
/// Total Return Index endpoint genuinely distinguishes them: for "strategy"
/// indices, `name` is a short internal code while `index_name` is the
/// display name.
fn build_request_body(
    name: &str,
    index_name: &str,
    from_date: NaiveDate,
    to_date: NaiveDate,
) -> String {
    let from_str = from_date.format("%d-%b-%Y");
    let to_str = to_date.format("%d-%b-%Y");
    let cinfo = format!(
        "{{'name': '{name}', 'startDate': '{from_str}', 'endDate': '{to_str}', 'indexName': '{index_name}'}}"
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

    /// Fetches daily P/E, P/B and dividend yield for an index between
    /// `from_date` and `to_date` (inclusive). Same chunking/concurrency and
    /// empty-result behavior as `index_history_raw`.
    pub async fn index_pe_history_raw(
        &self,
        name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<IndexPeRow>> {
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
                history.fetch_pe_chunk(&name, chunk_start, chunk_end).await
            }));
        }

        let mut rows = Vec::new();
        for task in tasks {
            rows.extend(task.await??);
        }

        Ok(rows)
    }

    /// Fetches P/E history the same way as `index_pe_history_raw`, then
    /// writes it as a CSV file into `dest` (a directory). Returns the path
    /// written.
    pub async fn index_pe_history_csv(
        &self,
        name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self.index_pe_history_raw(name, from_date, to_date).await?;

        let file_name = format!("{name}-pe-{from_date}-{to_date}.csv");
        let path = dest.join(file_name);

        let mut writer = csv::Writer::from_path(&path)?;
        for row in &rows {
            writer.serialize(row)?;
        }
        writer.flush()?;

        Ok(path)
    }

    /// Fetches daily Total Return Index values between `from_date` and
    /// `to_date` (inclusive). `name` and `index_name` are the same value
    /// for standard indices (e.g. both `"NIFTY 50"`); for strategy indices,
    /// `name` is the short internal code and `index_name` the display name -
    /// see `build_request_body`. Same chunking/concurrency and empty-result
    /// behavior as `index_history_raw`.
    pub async fn index_tri_history_raw(
        &self,
        name: &str,
        index_name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<IndexTriRow>> {
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

        let mut tasks = Vec::new();
        for (chunk_start, chunk_end) in break_into_month_chunks(from_date, to_date) {
            let history = self.clone();
            let name = name.to_string();
            let index_name = index_name.to_string();
            let semaphore = Arc::clone(&semaphore);

            tasks.push(tokio::spawn(async move {
                let _permit = semaphore
                    .acquire_owned()
                    .await
                    .map_err(|e| Error::Parse(format!("semaphore closed unexpectedly: {e}")))?;
                history
                    .fetch_tri_chunk(&name, &index_name, chunk_start, chunk_end)
                    .await
            }));
        }

        let mut rows = Vec::new();
        for task in tasks {
            rows.extend(task.await??);
        }

        Ok(rows)
    }

    /// Fetches TRI history the same way as `index_tri_history_raw`, then
    /// writes it as a CSV file into `dest` (a directory). Returns the path
    /// written.
    pub async fn index_tri_history_csv(
        &self,
        name: &str,
        index_name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self
            .index_tri_history_raw(name, index_name, from_date, to_date)
            .await?;

        let file_name = format!("{name}-tri-{from_date}-{to_date}.csv");
        let path = dest.join(file_name);

        let mut writer = csv::Writer::from_path(&path)?;
        for row in &rows {
            writer.serialize(row)?;
        }
        writer.flush()?;

        Ok(path)
    }

    /// Returns the top-level index categories (e.g. "Equity", "Fixed
    /// Income", "Multi Asset").
    pub async fn index_type_list(&self) -> Result<Vec<String>> {
        // No real body needed, but it can't be truly empty either: reqwest
        // omits Content-Length entirely for a zero-byte body rather than
        // sending "Content-Length: 0" (confirmed by testing both against
        // the live endpoint), and niftyindices' server rejects a request
        // with no Content-Length header at all with 411 (Length Required) -
        // even though the identical request via curl (which does send
        // "Content-Length: 0") succeeds. A single space is enough.
        let request = self
            .post_request("/BackPage/gethistoricaltypedata1")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(" ");
        fetch_discovery_list(request).await
    }

    /// Returns the sub-categories under `index_type` for a given
    /// `index_group` (e.g. `"Historical Index Data"`, `"Total returns Index
    /// Values "`, `"P/E, P/B & Div.Yield values"` - note the Python
    /// original's trailing spaces in some of these; niftyindices' own data
    /// has them).
    pub async fn index_subtype_list(
        &self,
        index_type: &str,
        index_group: &str,
    ) -> Result<Vec<String>> {
        let body = serde_json::json!({
            "cinfo": { "indextype": index_type, "indexgroup": index_group }
        })
        .to_string();
        let request = self
            .post_request("/BackPage/gethistoricaltypeSubindexdata")
            .header("Content-Type", "application/json; charset=UTF-8")
            .body(body);
        fetch_discovery_list(request).await
    }

    /// Returns the index names under `index_type` (a sub-category from
    /// `index_subtype_list`) for a given `index_group`. Unlike every other
    /// niftyindices request in this module, this endpoint wants a plain
    /// form-urlencoded body, not JSON at all - confirmed live.
    pub async fn index_name_list(
        &self,
        index_type: &str,
        index_group: &str,
    ) -> Result<Vec<String>> {
        let request = self
            .post_request("/BackPage/gethistoricaltypeindexdata")
            .form(&[
                ("cinfo[indextype]", index_type),
                ("cinfo[indexgroup]", index_group),
            ]);
        fetch_discovery_list(request).await
    }

    /// A `POST` to `path` on niftyindices.com with the headers every
    /// endpoint in this module needs. Callers add their own body/content
    /// type on top - the three discovery endpoints each want something
    /// different (no body, JSON, or form-urlencoded).
    fn post_request(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{BASE_URL}{path}"))
            .header("X-Requested-With", "XMLHttpRequest")
            .header("Referer", BASE_URL)
            .header("Origin", BASE_URL)
    }

    /// Fetches one chunk directly from niftyindices.com in a single request.
    /// Callers should use `index_history_raw`, which chunks long ranges.
    async fn fetch_chunk(
        &self,
        name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<IndexHistoryRow>> {
        self.fetch_index_json(
            "/BackPage/getHistoricaldatatabletoString",
            name,
            name,
            from_date,
            to_date,
        )
        .await
    }

    async fn fetch_pe_chunk(
        &self,
        name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<IndexPeRow>> {
        self.fetch_index_json(
            "/BackPage/getpepbHistoricaldataDBtoString",
            name,
            name,
            from_date,
            to_date,
        )
        .await
    }

    async fn fetch_tri_chunk(
        &self,
        name: &str,
        index_name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<IndexTriRow>> {
        self.fetch_index_json(
            "/BackPage/getTotalReturnIndexString",
            name,
            index_name,
            from_date,
            to_date,
        )
        .await
    }

    /// Shared body for `fetch_chunk`/`fetch_pe_chunk`/`fetch_tri_chunk`:
    /// these three history-style endpoints only differ in URL path and
    /// response shape, so `T` is generic over whichever row type the
    /// caller wants deserialized.
    async fn fetch_index_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        name: &str,
        index_name: &str,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<T>> {
        let body = build_request_body(name, index_name, from_date, to_date);

        let response = self
            .post_request(path)
            .header("Content-Type", "application/json; charset=UTF-8")
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
            .map_err(|e| Error::Parse(format!("could not parse niftyindices response: {e}")))
    }
}

/// Discovery endpoints (`index_type_list`/`index_subtype_list`/
/// `index_name_list`) all return the same mostly-null shape; only
/// `indextype` ever carries the value we actually want.
#[derive(Debug, Deserialize)]
struct DiscoveryItem {
    indextype: Option<String>,
}

async fn fetch_discovery_list(request: reqwest::RequestBuilder) -> Result<Vec<String>> {
    let response = request.send().await?;

    match response.status() {
        StatusCode::OK => {}
        status => return Err(Error::UnexpectedStatus(status)),
    }

    let items: Vec<DiscoveryItem> = response
        .json()
        .await
        .map_err(|e| Error::Parse(format!("could not parse niftyindices response: {e}")))?;

    Ok(items
        .into_iter()
        .filter_map(|item| item.indextype)
        .collect())
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
        let body = build_request_body("NIFTY 50", "NIFTY 50", date(2024, 8, 1), date(2024, 8, 5));
        assert_eq!(
            body,
            r#"{"cinfo":"{'name': 'NIFTY 50', 'startDate': '01-Aug-2024', 'endDate': '05-Aug-2024', 'indexName': 'NIFTY 50'}"}"#
        );
    }

    #[test]
    fn builds_cinfo_with_different_name_and_index_name_for_strategy_indices() {
        let body = build_request_body(
            "NIFTYQLV",
            "NIFTY100 QUALITY 30",
            date(2024, 8, 1),
            date(2024, 8, 5),
        );
        assert_eq!(
            body,
            r#"{"cinfo":"{'name': 'NIFTYQLV', 'startDate': '01-Aug-2024', 'endDate': '05-Aug-2024', 'indexName': 'NIFTY100 QUALITY 30'}"}"#
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

    // Real response captured from niftyindices' P/E endpoint for NIFTY 50.
    // Note the date field is "DATE" here - "HistoricalDate" for OHLC,
    // "Date" for TRI below. Three different key names, same site.
    const SAMPLE_PE_RESPONSE: &str = r#"[{"RequestNumber":"pepb639","Index Name":"Nifty 50",
        "pe":"22.38","pb":"4.05","divYield":"1.22","DATE":"05 Aug 2024"}]"#;

    #[test]
    fn deserializes_real_pe_response_shape() {
        let rows: Vec<IndexPeRow> = serde_json::from_str(SAMPLE_PE_RESPONSE).unwrap();

        assert_eq!(rows[0].index_name, "Nifty 50");
        assert_eq!(rows[0].date, date(2024, 8, 5));
        assert_eq!(rows[0].pe, 22.38);
        assert_eq!(rows[0].pb, 4.05);
        assert_eq!(rows[0].div_yield, 1.22);
    }

    // Real response captured from niftyindices' TRI endpoint for NIFTY 50.
    const SAMPLE_TRI_RESPONSE: &str = r#"[{"RequestNumber":"TRI639","Index Name":"Nifty 50",
        "Date":"05 Aug 2024","TotalReturnsIndex":"35646.37","NTR_Value":"32209.37"}]"#;

    #[test]
    fn deserializes_real_tri_response_shape() {
        let rows: Vec<IndexTriRow> = serde_json::from_str(SAMPLE_TRI_RESPONSE).unwrap();

        assert_eq!(rows[0].index_name, "Nifty 50");
        assert_eq!(rows[0].date, date(2024, 8, 5));
        assert_eq!(rows[0].total_returns_index, 35646.37);
        assert_eq!(rows[0].ntr_value, 32209.37);
    }

    #[test]
    fn pe_serializing_uses_clean_field_names_not_nse_names() {
        let rows: Vec<IndexPeRow> = serde_json::from_str(SAMPLE_PE_RESPONSE).unwrap();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&rows[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(header, "index_name,date,pe,pb,div_yield");
    }

    #[test]
    fn tri_serializing_uses_clean_field_names_not_nse_names() {
        let rows: Vec<IndexTriRow> = serde_json::from_str(SAMPLE_TRI_RESPONSE).unwrap();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&rows[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(header, "index_name,date,total_returns_index,ntr_value");
    }

    // Real response captured from niftyindices' discovery endpoints - mostly
    // null fields, only `indextype` ever carries the value we want.
    const SAMPLE_DISCOVERY_RESPONSE: &str = r#"[
        {"category":null,"title":null,"documentname":null,"downloadUrl":null,
         "dateofindex":null,"formatdate":null,"TitleName":null,
         "indextype":"Equity","indexgroup":null,"Date":null},
        {"category":null,"title":null,"documentname":null,"downloadUrl":null,
         "dateofindex":null,"formatdate":null,"TitleName":null,
         "indextype":null,"indexgroup":null,"Date":null},
        {"category":null,"title":null,"documentname":null,"downloadUrl":null,
         "dateofindex":null,"formatdate":null,"TitleName":null,
         "indextype":"Fixed Income","indexgroup":null,"Date":null}
    ]"#;

    #[test]
    fn discovery_items_with_null_indextype_are_filtered_out() {
        let items: Vec<DiscoveryItem> = serde_json::from_str(SAMPLE_DISCOVERY_RESPONSE).unwrap();
        let names: Vec<String> = items
            .into_iter()
            .filter_map(|item| item.indextype)
            .collect();

        assert_eq!(names, vec!["Equity", "Fixed Income"]);
    }
}
