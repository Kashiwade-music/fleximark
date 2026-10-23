//! Role: Validate JSON integer values without imposing limits on native domain types.
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegerOutOfRange;

impl fmt::Display for IntegerOutOfRange {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "integer is outside the JavaScript safe range")
    }
}

impl std::error::Error for IntegerOutOfRange {}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct JsSafeI64(i64);

impl JsSafeI64 {
    pub const MIN: i64 = -MAX_SAFE_INTEGER;
    pub const MAX: i64 = MAX_SAFE_INTEGER;

    pub const fn new(value: i64) -> Result<Self, IntegerOutOfRange> {
        if value >= Self::MIN && value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(IntegerOutOfRange)
        }
    }

    pub const fn get(self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for JsSafeI64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = <i64>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl TryFrom<i64> for JsSafeI64 {
    type Error = IntegerOutOfRange;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<JsSafeI64> for i64 {
    fn from(value: JsSafeI64) -> Self {
        value.get()
    }
}

impl PartialEq<i64> for JsSafeI64 {
    fn eq(&self, other: &i64) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<i64> for JsSafeI64 {
    fn partial_cmp(&self, other: &i64) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl PartialEq<JsSafeI64> for i64 {
    fn eq(&self, other: &JsSafeI64) -> bool {
        *self == other.0
    }
}

impl PartialOrd<JsSafeI64> for i64 {
    fn partial_cmp(&self, other: &JsSafeI64) -> Option<std::cmp::Ordering> {
        self.partial_cmp(&other.0)
    }
}

impl fmt::Display for JsSafeI64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(feature = "contract-schema")]
impl schemars::JsonSchema for JsSafeI64 {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        stringify!(JsSafeI64).into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::Schema::try_from(serde_json::json!({
            "type": "integer",
            "minimum": -MAX_SAFE_INTEGER,
            "maximum": MAX_SAFE_INTEGER
        }))
        .expect("integer schema is an object")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct JsSafeU64(u64);

impl JsSafeU64 {
    pub const MIN: u64 = 0;
    pub const MAX: u64 = MAX_SAFE_INTEGER as u64;

    pub const fn new(value: u64) -> Result<Self, IntegerOutOfRange> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(IntegerOutOfRange)
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for JsSafeU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = <u64>::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

impl TryFrom<u64> for JsSafeU64 {
    type Error = IntegerOutOfRange;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<JsSafeU64> for u64 {
    fn from(value: JsSafeU64) -> Self {
        value.get()
    }
}

impl PartialEq<u64> for JsSafeU64 {
    fn eq(&self, other: &u64) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<u64> for JsSafeU64 {
    fn partial_cmp(&self, other: &u64) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

impl PartialEq<JsSafeU64> for u64 {
    fn eq(&self, other: &JsSafeU64) -> bool {
        *self == other.0
    }
}

impl PartialOrd<JsSafeU64> for u64 {
    fn partial_cmp(&self, other: &JsSafeU64) -> Option<std::cmp::Ordering> {
        self.partial_cmp(&other.0)
    }
}

impl fmt::Display for JsSafeU64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[cfg(feature = "contract-schema")]
impl schemars::JsonSchema for JsSafeU64 {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> std::borrow::Cow<'static, str> {
        stringify!(JsSafeU64).into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::Schema::try_from(serde_json::json!({
            "type": "integer",
            "minimum": 0,
            "maximum": MAX_SAFE_INTEGER as u64
        }))
        .expect("integer schema is an object")
    }
}
impl From<u32> for JsSafeI64 {
    fn from(value: u32) -> Self {
        Self(i64::from(value))
    }
}

impl From<u32> for JsSafeU64 {
    fn from(value: u32) -> Self {
        Self(u64::from(value))
    }
}

impl TryFrom<JsSafeU64> for usize {
    type Error = std::num::TryFromIntError;

    fn try_from(value: JsSafeU64) -> Result<Self, Self::Error> {
        Self::try_from(value.get())
    }
}

impl TryFrom<JsSafeU64> for u32 {
    type Error = std::num::TryFromIntError;

    fn try_from(value: JsSafeU64) -> Result<Self, Self::Error> {
        Self::try_from(value.get())
    }
}

impl TryFrom<JsSafeI64> for u64 {
    type Error = std::num::TryFromIntError;

    fn try_from(value: JsSafeI64) -> Result<Self, Self::Error> {
        Self::try_from(value.get())
    }
}

#[cfg(test)]
mod tests {
    //! Role: Verify integer precision and bounds at the JSON boundary.
    use super::*;

    #[test]
    fn javascript_safe_integer_boundaries_round_trip_and_reject_overflow() {
        for value in [-MAX_SAFE_INTEGER, MAX_SAFE_INTEGER] {
            let safe: JsSafeI64 = serde_json::from_str(&value.to_string()).unwrap();
            assert_eq!(safe, value);
            assert_eq!(serde_json::to_string(&safe).unwrap(), value.to_string());
        }
        for value in [-MAX_SAFE_INTEGER - 1, MAX_SAFE_INTEGER + 1] {
            assert!(serde_json::from_str::<JsSafeI64>(&value.to_string()).is_err());
            assert!(JsSafeI64::new(value).is_err());
        }
        let maximum = MAX_SAFE_INTEGER as u64;
        for value in [0, maximum] {
            let safe: JsSafeU64 = serde_json::from_str(&value.to_string()).unwrap();
            assert_eq!(safe, value);
            assert_eq!(serde_json::to_string(&safe).unwrap(), value.to_string());
        }
        assert!(serde_json::from_str::<JsSafeU64>(&(maximum + 1).to_string()).is_err());
        assert!(JsSafeU64::new(maximum + 1).is_err());
    }
}
