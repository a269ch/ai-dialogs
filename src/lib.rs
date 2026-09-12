pub mod canonical;
pub mod cleaner;
pub mod cli;
pub mod error;
pub mod markdown;
pub mod models;
pub mod providers;
pub mod storage;
pub mod transfer;
pub mod tui;

#[cfg(test)]
#[path = "../tests/support/mod.rs"]
pub(crate) mod test_support;

pub use error::{AppError, Result};
