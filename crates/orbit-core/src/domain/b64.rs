//! Standard base64 for binary fields on the JSON command boundary.
//! Raw bytes would otherwise become a JSON array of numbers.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S: Serializer>(value: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error> {
    match value {
        Some(bytes) => serializer.serialize_str(&STANDARD.encode(bytes)),
        None => serializer.serialize_none(),
    }
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error> {
    let text = Option::<String>::deserialize(deserializer)?;
    text.map(|text| STANDARD.decode(text).map_err(serde::de::Error::custom))
        .transpose()
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        #[serde(default, skip_serializing_if = "Option::is_none", with = "super")]
        avatar: Option<Vec<u8>>,
    }

    #[test]
    fn optional_bytes_are_a_base64_string() {
        let sample = Sample {
            avatar: Some(b"\x89PNG".to_vec()),
        };
        let json = serde_json::to_string(&sample).unwrap();
        assert_eq!(json, r#"{"avatar":"iVBORw=="}"#);
        let parsed: Sample = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, sample);
        let empty = serde_json::to_string(&Sample { avatar: None }).unwrap();
        assert_eq!(empty, "{}");
        let missing: Sample = serde_json::from_str("{}").unwrap();
        assert_eq!(missing.avatar, None);
    }
}
