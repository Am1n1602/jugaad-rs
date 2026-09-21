#[cfg(feature = "dataframe")]
pub mod dataframe;
pub mod error;
pub mod nse;

pub use error::{Error, Result};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
