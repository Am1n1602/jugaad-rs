//! Date handling shared across NSE endpoints: chunking long ranges that
//! aren't reliable in a single request (stock and index history), sorting
//! chunked results back into order (see `sort_by_date_desc`), and parsing
//! the common `"17-Sep-2026"`-style date format several endpoints use.
//! Each was moved here once a second, identical consumer showed up - not
//! written speculatively ahead of that.

use chrono::{Datelike, Months, NaiveDate};
use serde::{Deserialize, Deserializer};

/// Finds the last day of the month `date` falls in.
fn end_of_month(date: NaiveDate) -> Option<NaiveDate> {
    let first_of_month = date.with_day(1)?;
    let first_of_next_month = first_of_month.checked_add_months(Months::new(1))?;
    first_of_next_month.pred_opt()
}

/// Splits `from_date..=to_date` into chunks that each stay within one
/// calendar month, capped at `to_date`.
pub(crate) fn break_into_month_chunks(
    from_date: NaiveDate,
    to_date: NaiveDate,
) -> Vec<(NaiveDate, NaiveDate)> {
    if (from_date.year(), from_date.month()) == (to_date.year(), to_date.month()) {
        return vec![(from_date, to_date)];
    }

    let mut chunks = Vec::new();
    let mut chunk_start = from_date;
    loop {
        let chunk_end = end_of_month(chunk_start).unwrap_or(to_date).min(to_date);
        chunks.push((chunk_start, chunk_end));
        if chunk_end >= to_date {
            break;
        }
        chunk_start = match chunk_end.succ_opt() {
            Some(next) => next,
            None => break,
        };
    }
    chunks
}

/// Sorts `rows` into descending (most-recent-first) date order, using
/// `date_of` to read each row's date.
///
/// NSE and niftyindices both return each *chunked* request's rows already
/// sorted - but in descending order internally, not ascending (confirmed
/// live for stock history, derivatives history, and niftyindices' history
/// endpoint). Since multi-month ranges are split into calendar-month
/// chunks and concatenated in ascending chunk order, naively concatenating
/// chunk results produces a "sawtooth" order - descending within each
/// chunk, but ascending chunk to chunk - rather than one correctly sorted
/// sequence. This normalizes that, regardless of how many chunks there
/// were or what order they completed in.
pub(crate) fn sort_by_date_desc<T>(rows: &mut [T], date_of: impl Fn(&T) -> NaiveDate) {
    rows.sort_by_key(|row| std::cmp::Reverse(date_of(row)));
}

/// Parses dates shaped like `"17-Sep-2026"`, used by stock/derivatives
/// history and the daily-reports metadata API.
pub(crate) fn deserialize_nse_date<'de, D>(
    deserializer: D,
) -> std::result::Result<NaiveDate, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    NaiveDate::parse_from_str(&raw, "%d-%b-%Y").map_err(serde::de::Error::custom)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn single_month_range_is_not_split() {
        let from = date(2024, 8, 1);
        let to = date(2024, 8, 15);
        assert_eq!(break_into_month_chunks(from, to), vec![(from, to)]);
    }

    #[test]
    fn multi_month_range_splits_on_month_boundaries() {
        let from = date(2024, 1, 15);
        let to = date(2024, 3, 10);

        assert_eq!(
            break_into_month_chunks(from, to),
            vec![
                (from, date(2024, 1, 31)),
                (date(2024, 2, 1), date(2024, 2, 29)), // 2024 is a leap year
                (date(2024, 3, 1), to),
            ]
        );
    }

    #[test]
    fn range_ending_on_a_month_boundary_has_no_trailing_empty_chunk() {
        let from = date(2024, 6, 15);
        let to = date(2024, 6, 30);
        assert_eq!(break_into_month_chunks(from, to), vec![(from, to)]);
    }

    #[test]
    fn sort_by_date_desc_orders_newest_first() {
        let mut rows = vec![date(2024, 1, 1), date(2024, 3, 1), date(2024, 2, 1)];
        sort_by_date_desc(&mut rows, |d| *d);
        assert_eq!(
            rows,
            vec![date(2024, 3, 1), date(2024, 2, 1), date(2024, 1, 1)]
        );
    }

    // Reproduces the real bug this function fixes: each "chunk" (Jan, then
    // Feb) is internally descending - NSE's actual behavior, confirmed
    // live - but chunks get concatenated in ascending chronological order.
    // Without sorting, that produces a "sawtooth" pattern rather than one
    // correctly ordered sequence.
    #[test]
    fn sort_by_date_desc_fixes_the_sawtooth_pattern_from_concatenated_chunks() {
        let mut rows = vec![
            date(2024, 1, 31),
            date(2024, 1, 15),
            date(2024, 1, 1),
            date(2024, 2, 29),
            date(2024, 2, 15),
            date(2024, 2, 1),
        ];
        sort_by_date_desc(&mut rows, |d| *d);
        assert_eq!(
            rows,
            vec![
                date(2024, 2, 29),
                date(2024, 2, 15),
                date(2024, 2, 1),
                date(2024, 1, 31),
                date(2024, 1, 15),
                date(2024, 1, 1),
            ]
        );
    }
}
