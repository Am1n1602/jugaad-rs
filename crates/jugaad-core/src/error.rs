use thiserror::Error;


/// Every error mode a client can face.
#[derive(Debug,Error)]
pub enum Error {
    #[error("network request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("NSE refused the request, reason can be bot protection or an invalid session")]
    Blocked,

    #[error("Data not published due to holiday,weekend or not released yet")]
    NoData,

    #[error("failed to parse response: {0}")]
    Parse(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

}

pub type Result<T> = std::result::Result<T,Error>;
