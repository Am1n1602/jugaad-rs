//! Downloads NSE's older, pre-Integrated-Filing "Regulation 33 Financial
//! Results" filings - the only source for machine-readable company
//! financials before SEBI's newer Integrated Filing framework existed
//! (roughly FY2024-25 onward). Not wrapped by Python's `jugaad_data`
//! today - only the newer framework is, via `corporate_integrated_filing`.
//!
//! Confirmed live: this endpoint's response shape genuinely differs by
//! `index` (segment) - `equities` and `sme` share the shape modeled here,
//! but `insurance` and `reitsinvits` each return a completely different,
//! unrelated set of fields through this same endpoint (no `isin`, no
//! `fromDate`/`toDate`, different field names entirely - `reitsinvits`'
//! shape is Integrated Filing data leaking through this URL, judging by
//! its `xbrl` filenames literally containing `INTEGRATED_FILING`).
//! `financial_results_raw` only supports `equities`/`sme`; other segments
//! will fail to deserialize. `debt` was untested - every query tried
//! returned zero rows even with the unfiltered-bulk-pull trick that found
//! real data immediately for every other segment.

use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};
use reqwest::StatusCode;
use serde::{Deserialize, Deserializer, Serialize};

use super::dates::deserialize_nse_date;
use super::http::{HttpClient, client_builder, with_retry};
use super::live::{download_bytes, filename_from_url, write_csv};
use crate::error::{Error, Result};

const BASE_URL: &str = "https://www.nseindia.com";

/// Whether a financial-results filing covers a full year or one quarter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultPeriod {
    Annual,
    Quarterly,
}

impl ResultPeriod {
    fn as_query_param(self) -> &'static str {
        match self {
            ResultPeriod::Annual => "Annual",
            ResultPeriod::Quarterly => "Quarterly",
        }
    }

    // Used to build a CSV filename - lowercase, matching this crate's
    // other filename conventions (see OptionChainKind::label).
    fn label(self) -> &'static str {
        match self {
            ResultPeriod::Annual => "annual",
            ResultPeriod::Quarterly => "quarterly",
        }
    }
}

/// Whether a filing's figures are consolidated (group-wide, including
/// subsidiaries) or non-consolidated (the entity alone).
///
/// An enum rather than a raw string on purpose: a naive substring check
/// for `"Consolidated"` also matches inside `"Non-Consolidated"` - a real
/// mistake confirmed to happen while investigating this endpoint. An enum
/// makes that class of bug impossible to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsolidationBasis {
    #[serde(rename = "Consolidated")]
    Consolidated,
    #[serde(rename = "Non-Consolidated")]
    NonConsolidated,
}

/// Whether a filing's figures are audited or not (quarterly results are
/// often unaudited/limited-review; annual results are almost always
/// audited).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditStatus {
    #[serde(rename = "Audited")]
    Audited,
    #[serde(rename = "Un-Audited", alias = "Unaudited")]
    UnAudited,
}

/// One Regulation-33 financial-results filing. Only `equities`/`sme`
/// segments are confirmed to match this shape - see the module docs.
#[derive(Debug, Serialize, Deserialize)]
pub struct FinancialResultRow {
    pub symbol: String,
    #[serde(rename(deserialize = "companyName"))]
    pub company_name: String,
    pub isin: String,
    pub consolidated: ConsolidationBasis,
    pub audited: AuditStatus,
    pub period: String,
    #[serde(rename(deserialize = "relatingTo"))]
    pub relating_to: String,
    // e.g. "Ind-AS New", "Non-Ind-AS", "NBFC-IND" - kept as a raw string
    // rather than an enum.
    #[serde(rename(deserialize = "indAs"))]
    pub ind_as: String,
    #[serde(
        rename(deserialize = "fromDate"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub from_date: NaiveDate,
    #[serde(
        rename(deserialize = "toDate"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub to_date: NaiveDate,
    #[serde(
        rename(deserialize = "filingDate"),
        deserialize_with = "deserialize_filing_date"
    )]
    pub filing_date: NaiveDateTime,
    #[serde(
        rename(deserialize = "xbrl"),
        deserialize_with = "deserialize_xbrl_url"
    )]
    pub xbrl_url: Option<String>,
    #[serde(rename(deserialize = "resultDetailedDataLink"))]
    pub result_detailed_data_link: Option<String>,
}

fn deserialize_filing_date<'de, D>(deserializer: D) -> std::result::Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    NaiveDateTime::parse_from_str(&raw, "%d-%b-%Y %H:%M").map_err(serde::de::Error::custom)
}

fn deserialize_xbrl_url<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    Ok(if raw.ends_with("/-") { None } else { Some(raw) })
}

#[derive(Debug, Clone)]
pub struct NseCorporateResults {
    client: HttpClient,
}

impl NseCorporateResults {
    pub fn new() -> Result<Self> {
        let client = client_builder().build()?;
        Ok(Self {
            client: with_retry(client),
        })
    }

    /// Fetches financial-results filings for `symbol` between `from_date`
    /// and `to_date` (inclusive), for the given `period`. `segment` is
    /// NSE's `index` query parameter - only `"equities"` and `"sme"` are
    /// confirmed to return the shape this crate models; other segments
    /// return different, incompatible response shapes through this same
    /// endpoint and will fail to deserialize (see the module docs).
    ///
    /// Deliberately doesn't expose NSE's `issuer` parameter: confirmed
    /// live, it's silently ignored when set to a ticker (returns every
    /// company's filings instead of a 200 error), so exposing it would
    /// just be a footgun. Returns an empty `Vec` for an unknown symbol or
    /// a range with no filings - confirmed live, NSE doesn't distinguish
    /// the two cases (same ambiguous-empty pattern used throughout this
    /// crate).
    pub async fn financial_results_raw(
        &self,
        segment: &str,
        symbol: &str,
        period: ResultPeriod,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<FinancialResultRow>> {
        let from_str = from_date.format("%d-%m-%Y").to_string();
        let to_str = to_date.format("%d-%m-%Y").to_string();

        let response = self
            .client
            .get(format!("{BASE_URL}/api/corporates-financial-results"))
            .query(&[
                ("index", segment),
                ("symbol", symbol),
                ("period", period.as_query_param()),
                ("from_date", from_str.as_str()),
                ("to_date", to_str.as_str()),
            ])
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        response
            .json()
            .await
            .map_err(|e| Error::Parse(format!("could not parse financial results response: {e}")))
    }

    /// Fetches filings the same way as `financial_results_raw`, then
    /// writes them as a CSV file into `dest` (a directory - the filename
    /// is derived from `segment`/`symbol`/`period`). Returns the path
    /// written.
    pub async fn financial_results_csv(
        &self,
        segment: &str,
        symbol: &str,
        period: ResultPeriod,
        from_date: NaiveDate,
        to_date: NaiveDate,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self
            .financial_results_raw(segment, symbol, period, from_date, to_date)
            .await?;
        let path = dest.join(format!(
            "{symbol}-{segment}-{}-financial-results.csv",
            period.label()
        ));
        write_csv(&rows, &path)
    }

    /// Downloads the raw XBRL instance document (XML) from
    /// `FinancialResultRow::xbrl_url`.
    pub async fn download_xbrl_raw(&self, xbrl_url: &str) -> Result<Vec<u8>> {
        download_bytes(&self.client, xbrl_url).await
    }

    /// Downloads the XBRL document the same way as `download_xbrl_raw`,
    /// then writes it into `dest` (a directory) under NSE's own filename
    /// for it - like `NseDailyReports::download_report_save`.
    pub async fn download_xbrl_save(&self, xbrl_url: &str, dest: &Path) -> Result<PathBuf> {
        let bytes = self.download_xbrl_raw(xbrl_url).await?;
        let path = dest.join(filename_from_url(xbrl_url));
        std::fs::write(&path, &bytes)?;
        Ok(path)
    }

    /// Downloads the raw HTML filing-detail page from
    /// `FinancialResultRow::result_detailed_data_link` - NSE's only
    /// available record for filings from before real XBRL existed
    /// (roughly pre-FY2018-19 for equities). This is an HTML page, not
    /// structured data. Only call this when `result_detailed_data_link`
    /// is `Some`.
    pub async fn download_result_html_raw(&self, url: &str) -> Result<Vec<u8>> {
        download_bytes(&self.client, url).await
    }

    /// Downloads the HTML page the same way as
    /// `download_result_html_raw`, then writes it into `dest` (a
    /// directory) under NSE's own filename for it - see
    /// `download_xbrl_save`. Returns the path written.
    pub async fn download_result_html_save(&self, url: &str, dest: &Path) -> Result<PathBuf> {
        let bytes = self.download_result_html_raw(url).await?;
        let path = dest.join(filename_from_url(url));
        std::fs::write(&path, &bytes)?;
        Ok(path)
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

    // Real response captured from NSE for TCS, equities segment, annual -
    // a recent filing with a real XBRL file.
    const SAMPLE_RECENT: &str =
        include_str!("../../tests/fixtures/corporate_results/sample_recent.json");

    #[test]
    fn deserializes_real_recent_filing_shape() {
        let rows: Vec<FinancialResultRow> = serde_json::from_str(SAMPLE_RECENT).unwrap();
        let row = &rows[0];

        assert_eq!(row.symbol, "TCS");
        assert_eq!(row.consolidated, ConsolidationBasis::NonConsolidated);
        assert_eq!(row.audited, AuditStatus::Audited);
        assert_eq!(row.from_date, date(2023, 4, 1));
        assert_eq!(row.to_date, date(2024, 3, 31));
        assert_eq!(
            row.filing_date,
            NaiveDate::from_ymd_opt(2024, 4, 12)
                .unwrap()
                .and_hms_opt(21, 4, 0)
                .unwrap()
        );
        assert!(row.xbrl_url.is_some());
        assert!(row.result_detailed_data_link.is_none());
    }

    // Real response captured from NSE for TCS's oldest annual filing
    // (FY2012-13) - placeholder XBRL, real HTML fallback link instead.
    const SAMPLE_OLD: &str = include_str!("../../tests/fixtures/corporate_results/sample_old.json");

    #[test]
    fn old_filing_has_placeholder_xbrl_normalized_to_none() {
        let rows: Vec<FinancialResultRow> = serde_json::from_str(SAMPLE_OLD).unwrap();
        let row = &rows[0];

        assert_eq!(row.xbrl_url, None);
        assert!(row.result_detailed_data_link.is_some());
    }

    // Real response shape captured from NSE for the sme segment - note
    // "Unaudited" (no hyphen), unlike equities' "Un-Audited".
    const SAMPLE_SME_UNAUDITED: &str =
        include_str!("../../tests/fixtures/corporate_results/sample_sme_unaudited.json");

    #[test]
    fn sme_unaudited_spelling_without_hyphen_still_parses() {
        let rows: Vec<FinancialResultRow> = serde_json::from_str(SAMPLE_SME_UNAUDITED).unwrap();
        assert_eq!(rows[0].audited, AuditStatus::UnAudited);
    }

    #[test]
    fn empty_array_response_deserializes_to_empty_vec() {
        let rows: Vec<FinancialResultRow> = serde_json::from_str("[]").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn serializing_uses_clean_field_names() {
        let rows: Vec<FinancialResultRow> = serde_json::from_str(SAMPLE_RECENT).unwrap();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&rows[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(
            header,
            "symbol,company_name,isin,consolidated,audited,period,relating_to,ind_as,from_date,to_date,filing_date,xbrl_url,result_detailed_data_link"
        );
    }

    #[test]
    fn filename_from_url_takes_the_last_path_segment() {
        assert_eq!(
            filename_from_url(
                "https://nsearchives.nseindia.com/corporate/xbrl/INDAS_104550_1090534.xml"
            ),
            "INDAS_104550_1090534.xml"
        );
        assert_eq!(
            filename_from_url(
                "https://nsearchives.nseindia.com/archives/financial_results/financial_res_TCS_104184.html"
            ),
            "financial_res_TCS_104184.html"
        );
    }
}
