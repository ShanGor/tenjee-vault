use crate::error::{VaultError, VaultResult};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

pub const MAX_ENTRIES: u32 = 100_000;
pub const META_LIMIT: u64 = 64 * 1024 * 1024;
pub const CHUNK: usize = 1024 * 1024;
pub const RESERVE: u64 = 32 * 1024 * 1024;

pub fn invalid(message: impl Into<String>) -> VaultError {
    VaultError::Validation(message.into())
}
pub mod wide {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: u32,
    pub path: Vec<String>,
    pub kind: Kind,
    #[serde(with = "wide")]
    pub size: u64,
    pub reason: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    File,
    Directory,
    Excluded,
}

// A deliberately portable filename contract. Unsupported names are explained,
// not silently rewritten. Duplicate legal names get an approved keep-both map.
pub fn component(name: &str) -> VaultResult<()> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && [
                "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³",
            ]
            .contains(&&stem[3..]));
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > 255
        || name.ends_with(['.', ' '])
        || reserved
        || name.chars().any(|c| {
            c.is_control() || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
        })
    {
        return Err(invalid(format!("Unsupported file name: {name:?}")));
    }
    Ok(())
}
pub fn path(parts: &[String]) -> VaultResult<String> {
    if parts.is_empty() || parts.len() > 128 {
        return Err(invalid("Folder depth exceeds 128 components"));
    }
    if key(&parts[..1]) == ".tenjee-partials" {
        return Err(invalid(
            "The batch staging directory name is reserved; rename the selected root",
        ));
    }
    let mut bytes = 0usize;
    for part in parts {
        component(part)?;
        bytes = bytes
            .checked_add(part.len() + 1)
            .ok_or_else(|| invalid("Path size overflow"))?;
    }
    if bytes > 4096 {
        return Err(invalid("Relative path exceeds 4 KiB"));
    }
    Ok(parts.join("/"))
}
pub fn key(parts: &[String]) -> String {
    parts
        .iter()
        .map(|s| s.nfc().flat_map(char::to_lowercase).collect::<String>())
        .collect::<Vec<_>>()
        .join("/")
}
pub fn bytes<T: Serialize>(value: &T) -> VaultResult<Vec<u8>> {
    serde_json::to_vec(value).map_err(|e| invalid(format!("Cannot encode transfer metadata: {e}")))
}
pub fn decode<T: serde::de::DeserializeOwned>(data: &[u8]) -> VaultResult<T> {
    serde_json::from_slice(data).map_err(|e| invalid(format!("Invalid transfer metadata: {e}")))
}
pub fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub batch: String,
    pub digest: String,
    pub files: u32,
    pub directories: u32,
    pub excluded: u32,
    pub entries: u32,
    #[serde(with = "wide")]
    pub total_bytes: u64,
    pub destination: String,
    pub completed: u32,
    pub phase: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_escapes_devices_streams_and_normalizes_collision_keys() {
        for name in [
            "..",
            "/tmp",
            "C:",
            "x:y",
            "CON.txt",
            "CON .txt",
            "COM¹.txt",
            "COM0",
            "LPT1",
            "name.",
            "a\\b",
            "\0",
        ] {
            assert!(component(name).is_err(), "{name:?}");
        }
        assert_eq!(key(&["É.txt".into()]), key(&["e\u{301}.TXT".into()]));
        assert!(component("文件.txt").is_ok());
    }
    #[test]
    fn wide_lengths_roundtrip_without_javascript_rounding() {
        let entry = Entry {
            id: 0,
            path: vec!["file".into()],
            kind: Kind::File,
            size: u64::MAX,
            reason: String::new(),
        };
        let encoded = bytes(&entry).unwrap();
        assert!(String::from_utf8_lossy(&encoded).contains("\"18446744073709551615\""));
        assert_eq!(decode::<Entry>(&encoded).unwrap(), entry);
    }
}
