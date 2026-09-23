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

macro_rules! safe_integer {
    ($name:ident, $primitive:ty, $minimum:expr, $maximum:expr) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name($primitive);

        impl $name {
            pub const MIN: $primitive = $minimum;
            pub const MAX: $primitive = $maximum;

            pub const fn new(value: $primitive) -> Result<Self, IntegerOutOfRange> {
                if value >= Self::MIN && value <= Self::MAX {
                    Ok(Self(value))
                } else {
                    Err(IntegerOutOfRange)
                }
            }

            pub const fn get(self) -> $primitive {
                self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = <$primitive>::deserialize(deserializer)?;
                Self::new(value).map_err(serde::de::Error::custom)
            }
        }

        impl TryFrom<$primitive> for $name {
            type Error = IntegerOutOfRange;

            fn try_from(value: $primitive) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for $primitive {
            fn from(value: $name) -> Self {
                value.get()
            }
        }

        impl PartialEq<$primitive> for $name {
            fn eq(&self, other: &$primitive) -> bool {
                self.0 == *other
            }
        }

        impl PartialOrd<$primitive> for $name {
            fn partial_cmp(&self, other: &$primitive) -> Option<std::cmp::Ordering> {
                self.0.partial_cmp(other)
            }
        }

        impl PartialEq<$name> for $primitive {
            fn eq(&self, other: &$name) -> bool {
                *self == other.0
            }
        }

        impl PartialOrd<$name> for $primitive {
            fn partial_cmp(&self, other: &$name) -> Option<std::cmp::Ordering> {
                self.partial_cmp(&other.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        #[cfg(feature = "contract-schema")]
        impl schemars::JsonSchema for $name {
            fn inline_schema() -> bool {
                true
            }

            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                schemars::json_schema!({
                    "type": "integer",
                    "minimum": $minimum,
                    "maximum": $maximum
                })
            }
        }
    };
}

safe_integer!(JsSafeI64, i64, -MAX_SAFE_INTEGER, MAX_SAFE_INTEGER);
safe_integer!(JsSafeU64, u64, 0, MAX_SAFE_INTEGER as u64);

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
