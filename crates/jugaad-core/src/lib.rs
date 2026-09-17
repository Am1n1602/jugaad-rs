pub mod error;

pub use error::{Error,Result};

pub fn version()-> &'static str {
    env!("CARGO_PKG_VERSION")
}
