//! Downloads NSE's corporate announcements feed - board meeting outcomes,
//! credit ratings, appointments/resignations, press releases, and similar
//! exchange disclosures. Distinct from `NseCorporateResults`, which covers
//! financial-results filings specifically.
//!
//! Confirmed live: NSE's real corporate-filings page uses seven `index`
//! (segment) values - `equities`, `sme`, `debt`, `mf`, `invitsreits`,
//! `municipalBond`, and `sse` (Social Stock Exchange, for registered
//! social enterprises/NPOs). The first six share the shape modeled by
//! `CorporateAnnouncementRow`; `sse` has a genuinely different one - real
//! data (253 rows checked over a 9-month window, e.g. "Sewa
//! International", some with their own `-SE`-suffixed symbols), just
//! different field names (`an_attach`/`an_desc`/`ann_date`/`comp_name`/...
//! instead of `attchmntFile`/`desc`/`an_dt`/`sm_name`/...) - modeled
//! separately as `SseAnnouncementRow` via `sse_announcements_raw` rather
//! than excluded.

use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Deserializer, Serialize};

use super::USER_AGENT;
use super::live::{download_bytes, filename_from_url, write_csv};
use crate::error::{Error, Result};

const BASE_URL: &str = "https://www.nseindia.com";

/// One corporate announcement/disclosure.
///
/// `symbol`/`isin` are `Option` - confirmed live, NSE only ever populates
/// both for the `equities` segment. `debt`/`municipalBond` (bonds have no
/// equity trading symbol) leave both `None`; `sme`/`mf`/`invitsreits` have
/// a `symbol` but leave `isin` `None`.

#[derive(Debug, Serialize, Deserialize)]
pub struct CorporateAnnouncementRow {
    pub symbol: Option<String>,
    #[serde(rename(deserialize = "sm_isin"))]
    pub isin: Option<String>,
    #[serde(rename(deserialize = "sm_name"))]
    pub company_name: String,
    // "desc" on the wire - kept as a raw string, not an enum: the category
    // list is long and open-ended.
    #[serde(rename(deserialize = "desc"))]
    pub category: String,
    #[serde(rename(deserialize = "attchmntText"))]
    pub description: String,
    // `null` for most segments, but the literal string `"-"` for `mf`.
    #[serde(
        rename(deserialize = "smIndustry"),
        deserialize_with = "deserialize_dash_or_null_as_none"
    )]
    pub industry: Option<String>,
    #[serde(rename(deserialize = "hasXbrl"))]
    pub has_xbrl: bool,
    #[serde(rename(deserialize = "attchmntFile"))]
    pub attachment_url: Option<String>,
    #[serde(rename(deserialize = "fileSize"))]
    pub file_size: Option<String>,
    #[serde(
        rename(deserialize = "an_dt"),
        deserialize_with = "deserialize_announcement_time"
    )]
    pub announcement_time: NaiveDateTime,
    #[serde(
        rename(deserialize = "seq_id"),
        deserialize_with = "deserialize_numeric_string"
    )]
    pub sequence_id: u64,
}

fn deserialize_dash_or_null_as_none<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.filter(|s| s != "-"))
}

fn deserialize_announcement_time<'de, D>(
    deserializer: D,
) -> std::result::Result<NaiveDateTime, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    NaiveDateTime::parse_from_str(&raw, "%d-%b-%Y %H:%M:%S").map_err(serde::de::Error::custom)
}

fn deserialize_numeric_string<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    raw.parse().map_err(serde::de::Error::custom)
}

/// One Social Stock Exchange (SSE) announcement - NSE's segment for
/// registered social enterprises (NPOs and for-profit social
/// enterprises), a genuinely different filer category from ordinary
/// listed companies. See the module docs for why this has its own type
/// rather than folding into `CorporateAnnouncementRow`.
///
/// `bm_Date`/`bm_date` (an exact duplicate pair, presumably "board
/// meeting date") were dropped: always `null` across all 253 rows
/// checked, and none of the 7 announcement categories seen (`Annual
/// Disclosure`, `Annual Impact Report`, `General Updates`, `Material
/// Events`, `Statement/Utilisation of Funds`) are board-meeting-related,
/// suggesting SSE filers may not go through that disclosure type at all -
/// same treatment as the other confirmed-always-null fields dropped from
/// `CorporateAnnouncementRow`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SseAnnouncementRow {
    // Only some social enterprises have one (e.g. "EF-SE") - confirmed
    // live, null for most (227 of 253 rows checked).
    pub symbol: Option<String>,
    #[serde(rename(deserialize = "comp_name"))]
    pub company_name: String,
    // "an_desc" on the wire - open-ended, not an enum, same reasoning as
    // `CorporateAnnouncementRow::category`.
    #[serde(rename(deserialize = "an_desc"))]
    pub category: String,
    pub text: String,
    #[serde(rename(deserialize = "hasXbrl"))]
    pub has_xbrl: bool,
    // Always present - confirmed live, unlike `CorporateAnnouncementRow`'s
    // `attachment_url`, which is null for the `mf` segment.
    #[serde(rename(deserialize = "an_attach"))]
    pub attachment_url: String,
    // Sometimes `null` even when `attachment_url` is present (17 of 253
    // rows checked) - the file exists but its size wasn't recorded.
    #[serde(rename(deserialize = "attFileSize"))]
    pub file_size: Option<String>,
    // "ann_date" on the wire - identical to "ann_Date" in every row
    // checked (an exact case-duplicate key), so only one is kept.
    // "ann_tstamp" (exchange processing time) and "diff_time" (the two
    // timestamps' difference) were dropped for the same reason as
    // `CorporateAnnouncementRow` drops `exchdisstime`/`difference`.
    #[serde(
        rename(deserialize = "ann_date"),
        deserialize_with = "deserialize_announcement_time"
    )]
    pub announcement_time: NaiveDateTime,
    #[serde(
        rename(deserialize = "seq_id"),
        deserialize_with = "deserialize_numeric_string"
    )]
    pub sequence_id: u64,
}

/// NSE's response is a plain array on success, but an unrecognized
/// `index` value returns a different envelope instead. Both are treated
/// as "no rows" here rather than an error.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AnnouncementsResponse<T> {
    Rows(Vec<T>),
    Other { data: Vec<T> },
}

impl<T> AnnouncementsResponse<T> {
    fn into_rows(self) -> Vec<T> {
        match self {
            AnnouncementsResponse::Rows(rows) => rows,
            AnnouncementsResponse::Other { data } => data,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NseCorporateAnnouncements {
    client: Client,
}

impl NseCorporateAnnouncements {
    pub fn new() -> Result<Self> {
        let client = Client::builder().user_agent(USER_AGENT).build()?;
        Ok(Self { client })
    }

    /// Fetches corporate announcements for `segment` between `from_date`
    /// and `to_date` (inclusive), optionally filtered to one `symbol`.
    /// `segment` is NSE's `index` query parameter - supports `"equities"`,
    /// `"sme"`, `"debt"`, `"mf"`, `"invitsreits"` and `"municipalBond"`
    /// (see `sse_announcements_raw` for the Social Stock Exchange, which
    /// uses a different response shape through this same endpoint).
    pub async fn corporate_announcements_raw(
        &self,
        segment: &str,
        symbol: Option<&str>,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<CorporateAnnouncementRow>> {
        let from_str = from_date.format("%d-%m-%Y").to_string();
        let to_str = to_date.format("%d-%m-%Y").to_string();

        let mut query = vec![
            ("index", segment),
            ("from_date", from_str.as_str()),
            ("to_date", to_str.as_str()),
        ];
        if let Some(symbol) = symbol {
            query.push(("symbol", symbol));
        }

        let response = self
            .client
            .get(format!("{BASE_URL}/api/corporate-announcements"))
            .query(&query)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: AnnouncementsResponse<CorporateAnnouncementRow> =
            response.json().await.map_err(|e| {
                Error::Parse(format!(
                    "could not parse corporate announcements response: {e}"
                ))
            })?;

        Ok(parsed.into_rows())
    }

    /// Fetches announcements the same way as `corporate_announcements_raw`,
    /// then writes them as a CSV file into `dest` (a directory - the
    /// filename is derived from `segment` and the date range). Returns the
    /// path written.
    pub async fn corporate_announcements_csv(
        &self,
        segment: &str,
        symbol: Option<&str>,
        from_date: NaiveDate,
        to_date: NaiveDate,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self
            .corporate_announcements_raw(segment, symbol, from_date, to_date)
            .await?;
        let file_name = match symbol {
            Some(symbol) => format!("{symbol}-{segment}-announcements-{from_date}-{to_date}.csv"),
            None => format!("{segment}-announcements-{from_date}-{to_date}.csv"),
        };
        let path = dest.join(file_name);
        write_csv(&rows, &path)
    }

    /// Fetches Social Stock Exchange announcements between `from_date`
    /// and `to_date` (inclusive), optionally filtered to one `symbol`.
    /// Hardcodes `index=sse` - the only value this shape is confirmed for
    /// (see the module docs). A separate method from
    /// `corporate_announcements_raw` because the response shape is
    /// genuinely different, not a variant of the same one.
    pub async fn sse_announcements_raw(
        &self,
        symbol: Option<&str>,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<SseAnnouncementRow>> {
        let from_str = from_date.format("%d-%m-%Y").to_string();
        let to_str = to_date.format("%d-%m-%Y").to_string();

        let mut query = vec![
            ("index", "sse"),
            ("from_date", from_str.as_str()),
            ("to_date", to_str.as_str()),
        ];
        if let Some(symbol) = symbol {
            query.push(("symbol", symbol));
        }

        let response = self
            .client
            .get(format!("{BASE_URL}/api/corporate-announcements"))
            .query(&query)
            .send()
            .await?;

        match response.status() {
            StatusCode::OK => {}
            StatusCode::FORBIDDEN => return Err(Error::Blocked),
            status => return Err(Error::UnexpectedStatus(status)),
        }

        let parsed: AnnouncementsResponse<SseAnnouncementRow> =
            response.json().await.map_err(|e| {
                Error::Parse(format!("could not parse SSE announcements response: {e}"))
            })?;

        Ok(parsed.into_rows())
    }

    /// Fetches SSE announcements the same way as `sse_announcements_raw`,
    /// then writes them as a CSV file into `dest` (a directory - the
    /// filename is derived from the date range). Returns the path
    /// written.
    pub async fn sse_announcements_csv(
        &self,
        symbol: Option<&str>,
        from_date: NaiveDate,
        to_date: NaiveDate,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self
            .sse_announcements_raw(symbol, from_date, to_date)
            .await?;
        let file_name = match symbol {
            Some(symbol) => format!("{symbol}-sse-announcements-{from_date}-{to_date}.csv"),
            None => format!("sse-announcements-{from_date}-{to_date}.csv"),
        };
        let path = dest.join(file_name);
        write_csv(&rows, &path)
    }

    /// Downloads the raw attachment from `CorporateAnnouncementRow::attachment_url`
    /// or `SseAnnouncementRow::attachment_url` - a PDF in every case
    /// checked live, but downloaded as-is rather than assumed, the same
    /// as `NseCorporateResults::download_xbrl_raw`/`download_result_html_raw`.
    pub async fn download_attachment_raw(&self, url: &str) -> Result<Vec<u8>> {
        download_bytes(&self.client, url).await
    }

    /// Downloads the attachment the same way as `download_attachment_raw`,
    /// then writes it into `dest` (a directory) under NSE's own filename
    /// for it - see `NseCorporateResults::download_xbrl_save`. Returns
    /// the path written.
    pub async fn download_attachment_save(&self, url: &str, dest: &Path) -> Result<PathBuf> {
        let bytes = self.download_attachment_raw(url).await?;
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

    // Real response captured live from NSE for the equities segment.
    const SAMPLE_EQUITIES: &str = r#"[{
        "an_dt":"21-Sep-2026 20:34:32","attFileSize":"1.22 MB",
        "attchmntFile":"https://nsearchives.nseindia.com/corporate/LUMINO123_21092026203419_Reg30Intimation.pdf",
        "attchmntText":"Lumino Industries Limited has informed the Exchange regarding a press release.",
        "bflag":null,"csvName":null,"desc":"Press Release","difference":"00:00:01",
        "dt":"21092026203432","exchdisstime":"21-Sep-2026 20:34:33","fileSize":"1.22 MB",
        "hasXbrl":true,"old_new":null,"orgid":null,"seq_id":"106786694","smIndustry":null,
        "sm_isin":"INE185Q01025","sm_name":"Lumino Industries Limited",
        "sort_date":"2026-09-21 20:34:32","symbol":"LUMINO"
    }]"#;

    #[test]
    fn deserializes_real_equities_shape() {
        let parsed: AnnouncementsResponse<CorporateAnnouncementRow> =
            serde_json::from_str(SAMPLE_EQUITIES).unwrap();
        let rows = parsed.into_rows();
        let row = &rows[0];

        assert_eq!(row.symbol, Some("LUMINO".to_string()));
        assert_eq!(row.isin, Some("INE185Q01025".to_string()));
        assert_eq!(row.category, "Press Release");
        assert!(row.has_xbrl);
        assert_eq!(row.sequence_id, 106_786_694);
        assert_eq!(
            row.announcement_time,
            date(2026, 9, 21).and_hms_opt(20, 34, 32).unwrap()
        );
        assert_eq!(row.industry, None);
    }

    // Real response shape captured live for the debt segment - no
    // symbol/isin (bonds have neither), uppercase month abbreviation.
    const SAMPLE_DEBT: &str = r#"[{
        "an_dt":"21-SEP-2026 20:05:59","attFileSize":"469.85 KB",
        "attchmntFile":"https://nsearchives.nseindia.com/content/debt/WDM/Grip_Payment.pdf",
        "attchmntText":"Payment confirmation.","bflag":null,"csvName":null,
        "desc":"Payment Confirmation","difference":"00:00:00","dt":"21092026200559",
        "exchdisstime":"21-SEP-2026 20:05:59","fileSize":"469.85 KB","hasXbrl":false,
        "old_new":null,"orgid":null,"seq_id":"106786000","smIndustry":null,
        "sm_isin":null,"sm_name":"Grip Prosperity Asset 1","sort_date":null,"symbol":null
    }]"#;

    #[test]
    fn debt_segment_has_no_symbol_or_isin_and_uppercase_month() {
        let parsed: AnnouncementsResponse<CorporateAnnouncementRow> =
            serde_json::from_str(SAMPLE_DEBT).unwrap();
        let row = &parsed.into_rows()[0];

        assert_eq!(row.symbol, None);
        assert_eq!(row.isin, None);
        assert_eq!(
            row.announcement_time,
            date(2026, 9, 21).and_hms_opt(20, 5, 59).unwrap()
        );
    }

    // Real response shape captured live for the mf segment - no
    // attachment, and `smIndustry` sent as the literal string "-".
    const SAMPLE_MF: &str = r#"[{
        "an_dt":"21-Sep-2026 17:39:00","attFileSize":null,"attchmntFile":null,
        "attchmntText":"Net Asset Value update.","bflag":null,"csvName":null,
        "desc":"General Updates","difference":"00:00:00","dt":"21092026173900",
        "exchdisstime":"21-Sep-2026 17:39:00","fileSize":null,"hasXbrl":false,
        "old_new":null,"orgid":null,"seq_id":"106785000","smIndustry":"-",
        "sm_isin":null,"sm_name":"DSP BSE Sensex ETF","sort_date":"2026-09-21 17:39:00",
        "symbol":"SENSEXADD"
    }]"#;

    #[test]
    fn mf_segment_normalizes_dash_industry_to_none_and_allows_null_attachment() {
        let parsed: AnnouncementsResponse<CorporateAnnouncementRow> =
            serde_json::from_str(SAMPLE_MF).unwrap();
        let row = &parsed.into_rows()[0];

        assert_eq!(row.industry, None);
        assert_eq!(row.attachment_url, None);
        assert_eq!(row.file_size, None);
        assert_eq!(row.symbol, Some("SENSEXADD".to_string()));
    }

    // An unrecognized `index` value returns this envelope instead of a
    // plain array - confirmed live.
    #[test]
    fn no_data_envelope_is_treated_as_empty() {
        let parsed: AnnouncementsResponse<CorporateAnnouncementRow> =
            serde_json::from_str(r#"{"data":[],"msg":"no data found"}"#).unwrap();
        assert!(parsed.into_rows().is_empty());
    }

    #[test]
    fn empty_array_response_deserializes_to_empty_vec() {
        let parsed: AnnouncementsResponse<CorporateAnnouncementRow> =
            serde_json::from_str("[]").unwrap();
        assert!(parsed.into_rows().is_empty());
    }

    #[test]
    fn serializing_uses_clean_field_names() {
        let parsed: AnnouncementsResponse<CorporateAnnouncementRow> =
            serde_json::from_str(SAMPLE_EQUITIES).unwrap();
        let rows = parsed.into_rows();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&rows[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(
            header,
            "symbol,isin,company_name,category,description,industry,has_xbrl,attachment_url,file_size,announcement_time,sequence_id"
        );
    }

    // Real response shape captured live for the sse segment - note the
    // completely different field names from every other segment.
    const SAMPLE_SSE: &str = r#"[{
        "an_attach":"https://nsearchives.nseindia.com/corporate/500003401_19092026173745_Annexurespdf.pdf",
        "an_desc":"General Updates","ann_Date":"19-Sep-2026 17:37:00",
        "ann_date":"19-Sep-2026 17:37:00","ann_tstamp":"19-Sep-2026 17:37:50",
        "attFileSize":"571.59 KB","bm_Date":null,"bm_date":null,
        "comp_name":"Sewa International","diff_time":"00:00:50","hasXbrl":true,
        "seq_id":"500003401","symbol":null,"text":"Renewal of registration"
    }]"#;

    #[test]
    fn deserializes_real_sse_shape() {
        let parsed: AnnouncementsResponse<SseAnnouncementRow> =
            serde_json::from_str(SAMPLE_SSE).unwrap();
        let row = &parsed.into_rows()[0];

        assert_eq!(row.company_name, "Sewa International");
        assert_eq!(row.category, "General Updates");
        assert_eq!(row.text, "Renewal of registration");
        assert!(row.has_xbrl);
        assert_eq!(row.symbol, None);
        assert_eq!(row.sequence_id, 500_003_401);
        assert_eq!(
            row.announcement_time,
            date(2026, 9, 19).and_hms_opt(17, 37, 0).unwrap()
        );
    }

    // Real response shape captured live: a social enterprise with its own
    // `-SE`-suffixed symbol, and an attachment with no recorded file size.
    const SAMPLE_SSE_WITH_SYMBOL: &str = r#"[{
        "an_attach":"https://nsearchives.nseindia.com/corporate/EFSE_annual.pdf",
        "an_desc":"Annual Disclosure","ann_Date":"15-Jul-2026 12:00:00",
        "ann_date":"15-Jul-2026 12:00:00","ann_tstamp":"15-Jul-2026 12:00:10",
        "attFileSize":null,"bm_Date":null,"bm_date":null,
        "comp_name":"Ekalavya Foundation","diff_time":"00:00:10","hasXbrl":false,
        "seq_id":"500001000","symbol":"EF-SE","text":"Annual disclosure filing"
    }]"#;

    #[test]
    fn sse_symbol_and_null_file_size_with_present_attachment() {
        let parsed: AnnouncementsResponse<SseAnnouncementRow> =
            serde_json::from_str(SAMPLE_SSE_WITH_SYMBOL).unwrap();
        let row = &parsed.into_rows()[0];

        assert_eq!(row.symbol, Some("EF-SE".to_string()));
        assert_eq!(row.file_size, None);
        assert!(!row.attachment_url.is_empty());
    }
}
