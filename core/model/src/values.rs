//! Role: Validate canonical SHA-256 hashes and opaque asset references.
use std::{fmt, str::FromStr};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidValue;
impl fmt::Display for InvalidValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected a canonical SHA-256 hash or asset reference")
    }
}
impl std::error::Error for InvalidValue {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sha256Hex(String);
impl Sha256Hex {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<&str> for Sha256Hex {
    type Error = InvalidValue;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidValue)
        }
    }
}
impl FromStr for Sha256Hex {
    type Err = InvalidValue;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value)
    }
}
impl fmt::Display for Sha256Hex {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AssetRef(String);
impl AssetRef {
    pub fn from_hash(hash: Sha256Hex) -> Self {
        Self(format!("fleximark-asset:{hash}"))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<&str> for AssetRef {
    type Error = InvalidValue;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let hash =
            Sha256Hex::try_from(value.strip_prefix("fleximark-asset:").ok_or(InvalidValue)?)?;
        Ok(Self::from_hash(hash))
    }
}
impl FromStr for AssetRef {
    type Err = InvalidValue;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value)
    }
}
impl fmt::Display for AssetRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    //! Role: Verify hash and asset-reference canonical forms.
    use super::*;
    #[test]
    fn only_accepts_complete_lowercase_hashes() -> Result<(), InvalidValue> {
        let valid = "0123456789abcdef".repeat(4);
        let hash = Sha256Hex::try_from(valid.as_str())?;
        assert_eq!(hash.as_str(), valid);
        let reference = AssetRef::from_hash(hash);
        assert_eq!(AssetRef::try_from(reference.as_str())?, reference);
        for value in [
            "".to_owned(),
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
        ] {
            assert!(Sha256Hex::try_from(value.as_str()).is_err());
            assert!(AssetRef::try_from(format!("fleximark-asset:{value}").as_str()).is_err());
        }
        assert!(AssetRef::try_from(valid.as_str()).is_err());
        Ok(())
    }
}
