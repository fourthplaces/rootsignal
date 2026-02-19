pub mod config;
pub mod error;
pub mod quality;
pub mod safety;
pub mod types;

pub use config::Config;
pub use error::RootSignalError;
pub use quality::*;
pub use safety::*;
pub use types::*;

/// Normalize a name into a URL-safe slug: lowercase, strip non-alphanumeric
/// (keeping spaces), collapse whitespace, replace spaces with hyphens.
///
/// ```
/// assert_eq!(rootsignal_common::slugify("Lake Street Church"), "lake-street-church");
/// assert_eq!(rootsignal_common::slugify("Lake St. Church!!!"), "lake-st-church");
/// assert_eq!(rootsignal_common::slugify("  Multiple   Spaces  "), "multiple-spaces");
/// ```
pub fn slugify(name: &str) -> String {
    let lowered = name.to_lowercase();
    let cleaned: String = lowered
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<&str>>().join("-")
}
