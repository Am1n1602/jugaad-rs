//! Date-range chunking shared by NSE endpoints that aren't reliable over
//! long date ranges in a single request (currently: stock and index
//! history). Extracted here once a second, identical consumer showed up -
//! not written speculatively ahead of that.

use chrono::{Datelike, Months, NaiveDate};

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
}
