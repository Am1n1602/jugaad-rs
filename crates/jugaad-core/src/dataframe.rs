//! Converts this crate's row types into a `polars` `DataFrame` - enabled
//! by the `dataframe` Cargo feature.
//!
//! Every row type in this crate already implements `Serialize` (for CSV
//! export), so this reuses that instead of hand-writing a column builder
//! per type: rows are serialized to newline-delimited JSON in memory, then
//! handed to polars' own JSON reader to infer columns and build the
//! `DataFrame`.

use std::io::Cursor;

use polars::prelude::*;
use serde::Serialize;

/// Converts any slice of this crate's row types into a `DataFrame`, one
/// row per element. `T` only needs `Serialize` - every `*Row`/`*Quote`
/// type returned by a `_raw` fetcher already has it.
///
/// An empty slice returns an empty (zero-row, zero-column) `DataFrame`
/// rather than an error - several fetchers in this crate return an empty
/// `Vec` for an unremarkable reason (a weekend, an unknown symbol), and
/// polars' own JSON reader can't infer a schema with no rows to look at.
pub fn to_dataframe<T: Serialize>(rows: &[T]) -> PolarsResult<DataFrame> {
    if rows.is_empty() {
        return Ok(DataFrame::empty());
    }

    let mut buf = Vec::new();
    for row in rows {
        serde_json::to_writer(&mut buf, row).map_err(|e| {
            PolarsError::ComputeError(format!("could not serialize row: {e}").into())
        })?;
        buf.push(b'\n');
    }
    JsonReader::new(Cursor::new(buf))
        .with_json_format(JsonFormat::JsonLines)
        .finish()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::NaiveDate;
    use serde::Serialize;

    use super::to_dataframe;
    use crate::nse::StockHistoryRow;

    #[derive(Debug, Serialize)]
    struct SampleRow {
        name: &'static str,
        price: f64,
        volume: Option<u64>,
    }

    #[test]
    fn converts_rows_into_a_dataframe_with_matching_shape_and_columns() {
        let rows = [
            SampleRow {
                name: "SBIN",
                price: 996.2,
                volume: Some(100),
            },
            SampleRow {
                name: "TCS",
                price: 2119.8,
                volume: None,
            },
        ];

        let df = to_dataframe(&rows).unwrap();

        assert_eq!(df.height(), 2);
        assert_eq!(df.get_column_names(), ["name", "price", "volume"]);
    }

    #[test]
    fn empty_slice_produces_a_zero_row_dataframe() {
        let rows: Vec<SampleRow> = Vec::new();
        let df = to_dataframe(&rows).unwrap();

        assert_eq!(df.height(), 0);
    }

    #[test]
    fn converts_a_real_crate_row_type() {
        let rows = [StockHistoryRow {
            symbol: "SBIN".to_string(),
            series: "EQ".to_string(),
            date: NaiveDate::from_ymd_opt(2024, 8, 1).unwrap(),
            open: 828.0,
            high: 840.0,
            low: 825.0,
            prev_close: 826.0,
            ltp: 838.0,
            close: 837.15,
            vwap: 833.5,
            volume: 12_345_678,
            value: 1.0e9,
            trades: Some(54_321),
            delivery_qty: Some(6_000_000),
            delivery_pct: Some(48.6),
        }];

        let df = to_dataframe(&rows).unwrap();

        assert_eq!(df.height(), 1);
        assert!(df.column("symbol").unwrap().str().unwrap().get(0) == Some("SBIN"));
    }
}
