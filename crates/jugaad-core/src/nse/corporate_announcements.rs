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
//!
//! Also covers SEBI's newer "Integrated Filing" framework
//! (`corporate_integrated_filing_raw`) - a different endpoint
//! (`/api/integrated-filing-results`) on the same corporate-filings family
//! of pages, confirmed live to support both `"Integrated Filing-
//! Financials"` and `"Integrated Filing- Governance"` filing types through
//! one shared response shape.

use std::path::{Path, PathBuf};

use chrono::{NaiveDate, NaiveDateTime};
use reqwest::StatusCode;
use serde::{Deserialize, Deserializer, Serialize};

use super::dates::deserialize_nse_date;
use super::http::{HttpClient, client_builder, with_retry};
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

/// One SEBI Integrated Filing entry - a combined financial/governance
/// disclosure framework, newer than the older Regulation 33 filings
/// `NseCorporateResults` covers.
///
/// `company_name`/`security_name` (`cmName`/`smName` on the wire) look
/// like duplicates but aren't always identical - confirmed live, they
/// differ in casing on about 1% of rows (e.g. `"Ghcl Textiles Limited"`
/// vs `"GHCL Textiles Limited""`), so both are kept rather than dropping
/// one as redundant.
#[derive(Debug, Serialize, Deserialize)]
pub struct IntegratedFilingRow {
    pub symbol: Option<String>,
    #[serde(rename(deserialize = "cmName"))]
    pub company_name: String,
    #[serde(rename(deserialize = "smName"))]
    pub security_name: String,
    // "Integrated Filing- Financials" or "Integrated Filing- Governance" -
    // confirmed live as the two real values, kept as a free string since
    // NSE may add more over time (same reasoning as `category` above).
    #[serde(rename(deserialize = "type"))]
    pub filing_type: String,
    #[serde(rename(deserialize = "type_Sub"))]
    pub filing_sub_type: String,
    #[serde(
        rename(deserialize = "qe_Date"),
        deserialize_with = "deserialize_nse_date"
    )]
    pub period_ended: NaiveDate,
    // "Audited"/"Un-Audited" for the Financials type, `null` for
    // Governance - kept as NSE's own text label rather than inverted into
    // a bool, since it isn't a `"true"`/`"false"` wire value.
    pub audited: Option<String>,
    // "Standalone"/"Consolidated" for Financials, `null` for Governance.
    pub consolidated: Option<String>,
    #[serde(
        rename(deserialize = "broadcast_Date"),
        deserialize_with = "deserialize_announcement_time_opt"
    )]
    pub broadcast_time: Option<NaiveDateTime>,
    #[serde(
        rename(deserialize = "creation_Date"),
        deserialize_with = "deserialize_announcement_time"
    )]
    pub creation_time: NaiveDateTime,
    #[serde(
        rename(deserialize = "revised_Date"),
        deserialize_with = "deserialize_announcement_time_opt"
    )]
    pub revised_time: Option<NaiveDateTime>,
    #[serde(rename(deserialize = "revision_Remark"))]
    pub revision_remark: Option<String>,
    #[serde(rename(deserialize = "xbrl"))]
    pub xbrl_url: String,
    // Confirmed live: `null` on some Governance-type rows (14 of 36
    // checked over one window), unlike every other file-size field in
    // this module - not the always-populated string a smaller sample
    // suggested.
    #[serde(rename(deserialize = "xbrlFileSize"))]
    pub xbrl_file_size: Option<String>,
    #[serde(rename(deserialize = "ixbrl"))]
    pub ixbrl_url: String,
    #[serde(rename(deserialize = "ixbrlFileSize"))]
    pub ixbrl_file_size: Option<String>,
    // Confirmed live across 1000 real rows: `null` (188/1000), a dead
    // sentinel literally ending in `/null` when no PDF exists (674/1000),
    // or a genuinely real, working URL (138/1000). Both "no attachment"
    // cases are normalized to `None` here rather than leaking the dead
    // sentinel URL as if it were real - same treatment as the `"-"`
    // placeholder fixed on `FiftyTwoWeekRow`.
    #[serde(
        rename(deserialize = "pdf_attach"),
        deserialize_with = "deserialize_pdf_attachment_url"
    )]
    pub pdf_attachment_url: Option<String>,
    // Confirmed live: this is the real PDF's size (up to tens of MB) when
    // `pdf_attachment_url` is real, not a dead/always-zero field - an
    // earlier 5-row sample happened to have no real attachments and made
    // it look that way.
    #[serde(rename(deserialize = "attFileSize"))]
    pub pdf_attachment_file_size: Option<String>,
    #[serde(
        rename(deserialize = "seq_Id"),
        deserialize_with = "deserialize_numeric_string"
    )]
    pub sequence_id: u64,
}

fn deserialize_announcement_time_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<NaiveDateTime>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|s| {
        NaiveDateTime::parse_from_str(&s, "%d-%b-%Y %H:%M:%S").map_err(serde::de::Error::custom)
    })
    .transpose()
}

fn deserialize_pdf_attachment_url<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.filter(|url| !url.ends_with("/null")))
}

/// `/api/integrated-filing-results` always wraps its rows in this
/// envelope (unlike `/api/corporate-announcements`, which returns a bare
/// array on success) - confirmed live, even the "missing required `type`
/// param" error case includes an empty `data` array, so no untagged-enum
/// trick is needed here.
#[derive(Debug, Deserialize)]
struct IntegratedFilingResponse<T> {
    data: Vec<T>,
}

#[derive(Debug, Clone)]
pub struct NseCorporateAnnouncements {
    client: HttpClient,
}

impl NseCorporateAnnouncements {
    pub fn new() -> Result<Self> {
        let client = client_builder().build()?;
        Ok(Self {
            client: with_retry(client),
        })
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

    /// Fetches SEBI Integrated Filing entries between `from_date` and
    /// `to_date` (inclusive) for `filing_type` (e.g. `"Integrated Filing-
    /// Financials"` or `"Integrated Filing- Governance"` - confirmed live
    /// as the two real values). `symbol` and `segment` (NSE's `index`
    /// query param - e.g. `"equities"`/`"sme"`) are optional filters,
    /// both confirmed live to genuinely narrow the result set.
    ///
    /// Unlike `corporate_announcements_raw`, NSE doesn't actually require
    /// a date range here (a bare unfiltered call succeeds), but this
    /// still takes one to keep results bounded - a global fetch would
    /// otherwise page through NSE's entire history (26,000+ rows and
    /// growing). Auto-paginates internally at 1000 rows per page
    /// (confirmed live, no smaller cap) until a short page signals the
    /// end, so the caller gets one flat `Vec` regardless of how many
    /// pages that took.
    ///
    /// Two of Python's filters are deliberately not exposed here. NSE's
    /// `period_ended` (e.g. `"30-JUN-2026"`) is confirmed live to
    /// genuinely filter, but is left out to keep this signature under
    /// clippy's argument-count lint - `from_date`/`to_date` already cover
    /// the general time-narrowing need. `issuer` is dropped for a
    /// different reason: confirmed live to have no effect at all (a
    /// garbage value returns the exact same result as no filter), so it
    /// isn't a real filter to build an API around.
    pub async fn corporate_integrated_filing_raw(
        &self,
        filing_type: &str,
        symbol: Option<&str>,
        segment: Option<&str>,
        from_date: NaiveDate,
        to_date: NaiveDate,
    ) -> Result<Vec<IntegratedFilingRow>> {
        const PAGE_SIZE: usize = 1000;
        let from_str = from_date.format("%d-%m-%Y").to_string();
        let to_str = to_date.format("%d-%m-%Y").to_string();
        let page_size_str = PAGE_SIZE.to_string();

        let mut rows = Vec::new();
        let mut page = 1u32;
        loop {
            let page_str = page.to_string();
            let mut query = vec![
                ("type", filing_type),
                ("from_date", from_str.as_str()),
                ("to_date", to_str.as_str()),
                ("page", page_str.as_str()),
                ("size", page_size_str.as_str()),
            ];
            if let Some(symbol) = symbol {
                query.push(("symbol", symbol));
            }
            if let Some(segment) = segment {
                query.push(("index", segment));
            }

            let response = self
                .client
                .get(format!("{BASE_URL}/api/integrated-filing-results"))
                .query(&query)
                .send()
                .await?;

            match response.status() {
                StatusCode::OK => {}
                StatusCode::FORBIDDEN => return Err(Error::Blocked),
                status => return Err(Error::UnexpectedStatus(status)),
            }

            let parsed: IntegratedFilingResponse<IntegratedFilingRow> =
                response.json().await.map_err(|e| {
                    Error::Parse(format!("could not parse integrated filing response: {e}"))
                })?;

            let fetched = parsed.data.len();
            rows.extend(parsed.data);
            if fetched < PAGE_SIZE {
                break;
            }
            page += 1;
        }

        Ok(rows)
    }

    /// Fetches integrated filings the same way as
    /// `corporate_integrated_filing_raw`, then writes them as a CSV file
    /// into `dest` (a directory - the filename is derived from `symbol`,
    /// `filing_type`, and the date range). Returns the path written.
    pub async fn corporate_integrated_filing_csv(
        &self,
        filing_type: &str,
        symbol: Option<&str>,
        segment: Option<&str>,
        from_date: NaiveDate,
        to_date: NaiveDate,
        dest: &Path,
    ) -> Result<PathBuf> {
        let rows = self
            .corporate_integrated_filing_raw(filing_type, symbol, segment, from_date, to_date)
            .await?;
        // "Integrated Filing- Financials" -> "financials", matching NSE's
        // own two real filing-type values.
        let type_slug = filing_type
            .rsplit(' ')
            .next()
            .unwrap_or(filing_type)
            .to_lowercase();
        let file_name = match symbol {
            Some(symbol) => {
                format!("{symbol}-{type_slug}-integrated-filing-{from_date}-{to_date}.csv")
            }
            None => format!("{type_slug}-integrated-filing-{from_date}-{to_date}.csv"),
        };
        let path = dest.join(file_name);
        write_csv(&rows, &path)
    }

    /// Downloads the raw attachment from `CorporateAnnouncementRow::attachment_url`,
    /// `SseAnnouncementRow::attachment_url`, or any of
    /// `IntegratedFilingRow`'s `xbrl_url`/`ixbrl_url`/`pdf_attachment_url` -
    /// a PDF in every case checked live for the announcement URLs, but
    /// downloaded as-is rather than assumed, the same as
    /// `NseCorporateResults::download_xbrl_raw`/`download_result_html_raw`.
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
    const SAMPLE_EQUITIES: &str =
        include_str!("../../tests/fixtures/corporate_announcements/sample_equities.json");

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
    const SAMPLE_DEBT: &str =
        include_str!("../../tests/fixtures/corporate_announcements/sample_debt.json");

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
    const SAMPLE_MF: &str =
        include_str!("../../tests/fixtures/corporate_announcements/sample_mf.json");

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
    const SAMPLE_SSE: &str =
        include_str!("../../tests/fixtures/corporate_announcements/sample_sse.json");

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
    const SAMPLE_SSE_WITH_SYMBOL: &str =
        include_str!("../../tests/fixtures/corporate_announcements/sample_sse_with_symbol.json");

    #[test]
    fn sse_symbol_and_null_file_size_with_present_attachment() {
        let parsed: AnnouncementsResponse<SseAnnouncementRow> =
            serde_json::from_str(SAMPLE_SSE_WITH_SYMBOL).unwrap();
        let row = &parsed.into_rows()[0];

        assert_eq!(row.symbol, Some("EF-SE".to_string()));
        assert_eq!(row.file_size, None);
        assert!(!row.attachment_url.is_empty());
    }

    // Real response captured live from /api/integrated-filing-results for
    // the Financials type - a dead pdf_attach sentinel (ends in `/null`),
    // which should normalize to `None`.
    const SAMPLE_INTEGRATED_FILING_FINANCIALS: &str = include_str!(
        "../../tests/fixtures/corporate_announcements/sample_integrated_filing_financials.json"
    );

    #[test]
    fn deserializes_real_financials_shape_and_normalizes_dead_pdf_sentinel() {
        let parsed: IntegratedFilingResponse<IntegratedFilingRow> =
            serde_json::from_str(SAMPLE_INTEGRATED_FILING_FINANCIALS).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.symbol, Some("WINSOME".to_string()));
        assert_eq!(row.filing_type, "Integrated Filing- Financials");
        assert_eq!(row.filing_sub_type, "Original");
        assert_eq!(row.audited, Some("Un-Audited".to_string()));
        assert_eq!(row.period_ended, date(2026, 6, 30));
        assert_eq!(
            row.broadcast_time,
            Some(date(2026, 9, 23).and_hms_opt(19, 18, 3).unwrap())
        );
        assert_eq!(row.revised_time, None);
        assert_eq!(row.pdf_attachment_url, None);
        assert_eq!(row.sequence_id, 195_779);
    }

    // Real response captured live for the Governance type - `audited`/
    // `consolidated` are `null` (not applicable to governance filings),
    // and `pdf_attach` is a genuinely real, working URL here.
    const SAMPLE_INTEGRATED_FILING_GOVERNANCE: &str = include_str!(
        "../../tests/fixtures/corporate_announcements/sample_integrated_filing_governance.json"
    );

    #[test]
    fn deserializes_real_governance_shape_with_null_audited_and_real_pdf_url() {
        let parsed: IntegratedFilingResponse<IntegratedFilingRow> =
            serde_json::from_str(SAMPLE_INTEGRATED_FILING_GOVERNANCE).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.audited, None);
        assert_eq!(row.consolidated, None);
        assert_eq!(
            row.pdf_attachment_url,
            Some(
                "https://nsearchives.nseindia.com/corporate/HOVS_22092026134458_HGM-Clarification22Sept2026.pdf"
                    .to_string()
            )
        );
        assert_eq!(
            row.revised_time,
            Some(date(2026, 9, 23).and_hms_opt(18, 17, 8).unwrap())
        );
        assert_eq!(row.revision_remark, Some("Revised".to_string()));
    }

    // Real response captured live: a Governance-type row where
    // `xbrlFileSize`/`ixbrlFileSize` are `null`, not the always-populated
    // string a smaller sample suggested.
    const SAMPLE_INTEGRATED_FILING_NULL_FILE_SIZES: &str = include_str!(
        "../../tests/fixtures/corporate_announcements/sample_integrated_filing_null_file_sizes.json"
    );

    #[test]
    fn accepts_null_xbrl_and_ixbrl_file_sizes() {
        let parsed: IntegratedFilingResponse<IntegratedFilingRow> =
            serde_json::from_str(SAMPLE_INTEGRATED_FILING_NULL_FILE_SIZES).unwrap();
        let row = &parsed.data[0];

        assert_eq!(row.xbrl_file_size, None);
        assert_eq!(row.ixbrl_file_size, None);
        assert_eq!(row.pdf_attachment_url, None);
    }

    #[test]
    fn integrated_filing_row_serializes_with_clean_field_names() {
        let parsed: IntegratedFilingResponse<IntegratedFilingRow> =
            serde_json::from_str(SAMPLE_INTEGRATED_FILING_FINANCIALS).unwrap();
        let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
        writer.serialize(&parsed.data[0]).unwrap();
        let csv_text = String::from_utf8(writer.into_inner().unwrap()).unwrap();

        let header = csv_text.lines().next().unwrap();
        assert_eq!(
            header,
            "symbol,company_name,security_name,filing_type,filing_sub_type,period_ended,audited,consolidated,broadcast_time,creation_time,revised_time,revision_remark,xbrl_url,xbrl_file_size,ixbrl_url,ixbrl_file_size,pdf_attachment_url,pdf_attachment_file_size,sequence_id"
        );
    }

    // Full real Integrated Filing responses (1000 Financials rows, 500
    // Governance rows), captured live specifically because the small
    // hand-picked samples above only demonstrate one bug each (the dead
    // pdf_attach sentinel, the null file sizes) on a single row.
    // Deserializing the whole thing checks every row parses, not just the
    // one known-bad one - the actual regression guard against whatever
    // the next edge case turns out to be.
    const LARGE_INTEGRATED_FILING_FINANCIALS: &str = include_str!(
        "../../tests/fixtures/corporate_announcements/large_integrated_filing_financials.json"
    );
    const LARGE_INTEGRATED_FILING_GOVERNANCE: &str = include_str!(
        "../../tests/fixtures/corporate_announcements/large_integrated_filing_governance.json"
    );

    #[test]
    fn deserializes_full_real_integrated_filing_responses() {
        let financials: IntegratedFilingResponse<IntegratedFilingRow> =
            serde_json::from_str(LARGE_INTEGRATED_FILING_FINANCIALS).unwrap();
        let governance: IntegratedFilingResponse<IntegratedFilingRow> =
            serde_json::from_str(LARGE_INTEGRATED_FILING_GOVERNANCE).unwrap();

        assert_eq!(financials.data.len(), 1000);
        assert_eq!(governance.data.len(), 500);

        // Both known (at fixture-capture time) to contain all three
        // pdf_attach states / both null-file-size cases - confirms the
        // normalization holds across the full response, not just the one
        // row each smaller sample was built from.
        let real_pdf_count = financials
            .data
            .iter()
            .filter(|r| r.pdf_attachment_url.is_some())
            .count();
        assert!(real_pdf_count > 0);
        assert!(governance.data.iter().any(|r| r.xbrl_file_size.is_none()));
        assert!(governance.data.iter().any(|r| r.ixbrl_file_size.is_none()));
    }

    // Confirmed live: a missing/invalid `type` param returns this envelope
    // instead of real rows - still parses cleanly to an empty `Vec` since
    // `IntegratedFilingResponse` only requires `data`.
    #[test]
    fn missing_type_param_envelope_parses_to_empty_vec() {
        let parsed: IntegratedFilingResponse<IntegratedFilingRow> =
            serde_json::from_str(r#"{"data":[],"msg":"no data found"}"#).unwrap();
        assert!(parsed.data.is_empty());
    }
}
