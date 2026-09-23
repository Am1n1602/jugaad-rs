//! Runs every `jugaad` subcommand once against live NSE, for manual
//! spot-checking of what each command actually fetches. Hits real network
//! endpoints, so - like the rest of this workspace's live tests - it's
//! `#[ignore]`d by default. Run it explicitly with:
//!
//!     cargo test -p jugaad-cli -- --ignored --nocapture
//!
//! Output files land under `data/smoke_test` at the workspace root (`data/`
//! is already gitignored) so you can inspect the actual CSVs/data
//! afterward.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

struct CmdResult {
    label: String,
    ok: bool,
    detail: String,
}

fn run(bin: &Path, out_dir: &Path, args: &[&str]) -> CmdResult {
    let label = args.join(" ");
    match Command::new(bin).args(args).current_dir(out_dir).output() {
        Ok(output) => {
            let ok = output.status.success();
            let text = if ok { output.stdout } else { output.stderr };
            CmdResult {
                label,
                ok,
                detail: String::from_utf8_lossy(&text).trim().to_string(),
            }
        }
        Err(e) => CmdResult {
            label,
            ok: false,
            detail: format!("failed to spawn binary: {e}"),
        },
    }
}

/// Finds the first field in a CSV file that looks like a URL ending in one
/// of `exts` - used to chain a real URL from one command's output into the
/// next command (e.g. financial-results' `xbrl_url` into download-xbrl).
fn find_url_in_csv(path: &Path, exts: &[&str]) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.split([',', '\n', '\r'])
        .map(str::trim)
        .find(|field| field.starts_with("http") && exts.iter().any(|ext| field.ends_with(ext)))
        .map(str::to_string)
}

fn first_csv_in(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "csv"))
}

#[test]
#[ignore = "hits live NSE - run manually: cargo test -p jugaad-cli -- --ignored --nocapture"]
fn run_every_cli_command_live() {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_jugaad"));
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/smoke_test");
    std::fs::create_dir_all(&out_dir).unwrap();
    let out_dir = out_dir.canonicalize().unwrap();
    println!("Output directory: {}", out_dir.display());

    let mut results: Vec<CmdResult> = Vec::new();
    macro_rules! run_cmd {
        ($($arg:expr),+ $(,)?) => {{
            let r = run(&bin, &out_dir, &[$($arg),+]);
            println!("{} {}\n{}\n", if r.ok { "OK  " } else { "FAIL" }, r.label, r.detail);
            results.push(r);
        }};
    }

    run_cmd!("version");
    run_cmd!("bhavcopy", "2026-09-22", "--output", "bhavcopy");
    run_cmd!("bhavcopy-fo", "2026-09-22", "--output", "bhavcopy-fo");
    run_cmd!("full-bhavcopy", "2026-09-22", "--output", "full-bhavcopy");
    run_cmd!(
        "stock",
        "SBIN",
        "--from",
        "2026-08-01",
        "--to",
        "2026-08-31",
        "--output",
        "stock"
    );
    run_cmd!("bulk-deals", "--output", "bulk-deals.csv");
    run_cmd!("list-daily-reports", "--segment", "CM");
    run_cmd!(
        "daily-report",
        "CM-BULK-DEAL",
        "--segment",
        "CM",
        "--output",
        "daily-report"
    );
    run_cmd!(
        "index",
        "NIFTY 50",
        "--from",
        "2026-08-01",
        "--to",
        "2026-08-31",
        "--output",
        "index"
    );
    run_cmd!(
        "index-pe",
        "NIFTY 50",
        "--from",
        "2026-08-01",
        "--to",
        "2026-08-31",
        "--output",
        "index-pe"
    );
    run_cmd!(
        "index-tri",
        "NIFTY 50",
        "--from",
        "2026-08-01",
        "--to",
        "2026-08-31",
        "--output",
        "index-tri"
    );
    run_cmd!("index-types");
    run_cmd!(
        "index-subtypes",
        "--index-type",
        "Equity",
        "--index-group",
        "Historical Index Data"
    );
    run_cmd!(
        "index-names",
        "--index-type",
        "Broad Market Indices",
        "--index-group",
        "Historical Index Data"
    );
    run_cmd!(
        "derivatives",
        "NIFTY",
        "--from",
        "2026-08-01",
        "--to",
        "2026-08-05",
        "--expiry",
        "2026-08-25",
        "--instrument",
        "fut-idx",
        "--output",
        "derivatives"
    );
    run_cmd!("market-status", "--output", "market-status.csv");
    run_cmd!("index-snapshot", "--output", "index-snapshot.csv");
    run_cmd!("market-turnover", "--output", "market-turnover.csv");
    run_cmd!("live-fo", "--output", "live-fo.csv");
    run_cmd!("block-deal-session", "--output", "block-deal-session.csv");
    run_cmd!(
        "eq-derivative-turnover",
        "--output",
        "eq-derivative-turnover.csv"
    );
    run_cmd!("market-movers", "--output", "market-movers.csv");
    run_cmd!(
        "most-active-equities",
        "--output",
        "most-active-equities.csv"
    );
    run_cmd!("volume-gainers", "--output", "volume-gainers.csv");
    run_cmd!("fifty-two-week", "--output", "fifty-two-week.csv");
    run_cmd!("large-deals", "--output", "large-deals.csv");
    run_cmd!("stock-quote", "SBIN", "--output", "stock-quote");
    run_cmd!(
        "stock-chart",
        "SBIN",
        "--period",
        "1d",
        "--output",
        "stock-chart"
    );
    run_cmd!("derivative-quote", "NIFTY", "--output", "derivative-quote");
    run_cmd!("index-quote", "NIFTY 50", "--output", "index-quote");
    run_cmd!(
        "option-chain",
        "NIFTY",
        "--kind",
        "index",
        "--output",
        "option-chain"
    );
    run_cmd!(
        "currency-option-chain",
        "USDINR",
        "--output",
        "currency-option-chain"
    );
    run_cmd!(
        "financial-results",
        "TCS",
        "--period",
        "annual",
        "--from",
        "2012-01-01",
        "--to",
        "2024-12-31",
        "--output",
        "financial-results"
    );
    run_cmd!(
        "corporate-announcements",
        "--symbol",
        "SBIN",
        "--from",
        "2026-01-01",
        "--to",
        "2026-09-21",
        "--output",
        "corporate-announcements"
    );
    run_cmd!(
        "sse-announcements",
        "--from",
        "2026-01-01",
        "--to",
        "2026-09-21",
        "--output",
        "sse-announcements"
    );
    run_cmd!("holiday-list", "--output", "holiday-list.csv");
    run_cmd!("index-bhavcopy", "2026-09-22", "--output", "index-bhavcopy");
    run_cmd!("reg-details", "SBIN", "--output", "reg-details");
    run_cmd!("index-list", "SBIN");
    run_cmd!("symbol-meta", "SBIN", "--output", "symbol-meta");
    run_cmd!("symbol-name", "SBIN", "--output", "symbol-name");
    run_cmd!("yearwise-data", "SBIN", "--output", "yearwise-data");
    run_cmd!(
        "index-chart",
        "NIFTY 50",
        "--period",
        "1d",
        "--output",
        "index-chart"
    );

    // The three URL-taking commands need a real URL from an earlier
    // command's CSV output, so they're chained here instead of hardcoded.
    if let Some(csv) = first_csv_in(&out_dir.join("financial-results")) {
        if let Some(url) = find_url_in_csv(&csv, &[".xml"]) {
            run_cmd!("download-xbrl", url.as_str(), "--output", "download-xbrl");
        }
        if let Some(url) = find_url_in_csv(&csv, &[".html"]) {
            run_cmd!(
                "download-result-html",
                url.as_str(),
                "--output",
                "download-result-html"
            );
        }
    }
    let attachment_url = first_csv_in(&out_dir.join("corporate-announcements"))
        .and_then(|csv| find_url_in_csv(&csv, &[".pdf", ".xml", ".html"]))
        .or_else(|| {
            first_csv_in(&out_dir.join("sse-announcements"))
                .and_then(|csv| find_url_in_csv(&csv, &[".pdf", ".xml", ".html"]))
        });
    if let Some(url) = attachment_url {
        run_cmd!(
            "download-announcement-attachment",
            url.as_str(),
            "--output",
            "download-announcement-attachment"
        );
    }

    let failures: Vec<&CmdResult> = results.iter().filter(|r| !r.ok).collect();
    println!(
        "\n{} / {} commands succeeded",
        results.len() - failures.len(),
        results.len()
    );
    for f in &failures {
        println!("FAILED: {} -> {}", f.label, f.detail);
    }
    assert!(
        failures.is_empty(),
        "{} command(s) failed - see output above",
        failures.len()
    );
}
