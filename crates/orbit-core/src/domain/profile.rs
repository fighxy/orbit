use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const MAX_DISPLAY_NAME_CHARS: usize = 64;
pub const MAX_ABOUT_CHARS: usize = 140;

/// Public profile of the local account. Stored on this device; sharing it with
/// contacts arrives with contact exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub display_name: String,
    pub about: String,
    pub updated_at_ms: i64,
}

/// Trims and validates profile fields. The name is required and single-line.
pub fn normalize_profile(display_name: &str, about: &str) -> Result<(String, String)> {
    let name = display_name.trim();
    if name.is_empty() {
        return Err(Error::InvalidArgument("display name is empty".into()));
    }
    if name.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err(Error::InvalidArgument(format!(
            "display name exceeds {MAX_DISPLAY_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(Error::InvalidArgument(
            "display name contains control characters".into(),
        ));
    }
    let about = about.trim();
    if about.chars().count() > MAX_ABOUT_CHARS {
        return Err(Error::InvalidArgument(format!(
            "about exceeds {MAX_ABOUT_CHARS} characters"
        )));
    }
    if about.chars().any(|c| c.is_control() && c != '\n') {
        return Err(Error::InvalidArgument("about contains control characters".into()));
    }
    Ok((name.to_owned(), about.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_validates() {
        assert_eq!(
            normalize_profile("  Анна  ", " люблю звёзды \n").unwrap(),
            ("Анна".to_owned(), "люблю звёзды".to_owned())
        );
        assert!(normalize_profile("   ", "").is_err());
        assert!(normalize_profile("a\nb", "").is_err());
        assert!(normalize_profile(&"я".repeat(MAX_DISPLAY_NAME_CHARS), "").is_ok());
        assert!(normalize_profile(&"я".repeat(MAX_DISPLAY_NAME_CHARS + 1), "").is_err());
        assert!(normalize_profile("Анна", &"x".repeat(MAX_ABOUT_CHARS + 1)).is_err());
        assert!(normalize_profile("Анна", "line\nline").is_ok());
    }
}
