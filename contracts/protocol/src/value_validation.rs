//! Role: Enforce canonical hash and asset-reference strings at the wire boundary.
use serde::{Deserialize, Deserializer};
fn lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
pub(crate) fn sha256<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if !lowercase_sha256(&value) {
        return Err(serde::de::Error::custom(
            "expected 64 lowercase hexadecimal SHA-256 digits",
        ));
    }
    Ok(value)
}
pub(crate) fn asset_reference<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if !value
        .strip_prefix("fleximark-asset:")
        .is_some_and(lowercase_sha256)
    {
        return Err(serde::de::Error::custom(
            "expected a canonical fleximark asset reference",
        ));
    }
    Ok(value)
}
