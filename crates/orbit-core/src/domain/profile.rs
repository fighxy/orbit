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
    /// JPEG or PNG, at most 32 KiB. Omitted when the user has not set one.
    #[serde(default, skip_serializing_if = "Option::is_none", with = "super::b64")]
    pub avatar: Option<Vec<u8>>,
}

/// Keet's username rules: 4 to 31 characters, lowercase ASCII, digits and
/// underscore, with at least one digit. This is the name chosen at
/// registration. It is not a global directory.
pub fn normalize_username(raw: &str) -> Result<String> {
    let name = raw.trim();
    let len = name.chars().count();
    if !(4..=31).contains(&len) {
        return Err(Error::InvalidArgument("username length".into()));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(Error::InvalidArgument("username characters".into()));
    }
    if !name.bytes().any(|byte| byte.is_ascii_digit()) {
        return Err(Error::InvalidArgument("username needs a digit".into()));
    }
    Ok(name.to_owned())
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

    #[test]
    fn username_follows_the_registration_rules() {
        assert_eq!(normalize_username("  anya1 ").unwrap(), "anya1");
        assert_eq!(normalize_username("ab_3").unwrap(), "ab_3");
        assert!(normalize_username("anya").is_err());
        assert!(normalize_username("Anya1").is_err());
        assert!(normalize_username("ан1").is_err());
        assert!(normalize_username("abc").is_err());
        assert!(normalize_username(&format!("a1{}", "b".repeat(30))).is_err());
    }
}
