//! Output formatting for review results.

pub mod annotations;
pub mod json;
pub mod markdown;
pub mod terminal;

use crate::review::models::ReviewResult;

/// Trait for formatting review results.
pub trait OutputFormatter {
    /// Format and display the review result.
    fn format(&self, result: &ReviewResult) -> String;
}
