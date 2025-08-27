//! r#impl AHHHHHH

use serde::{Deserialize, Deserializer, de};

use super::NUDState;

// Case-insensitive parsing for NUDState via manual Deserialize
impl<'de> Deserialize<'de> for NUDState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = <&str as Deserialize>::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

#[allow(unused_imports)]
pub use crate::utils::parse::mac::{des_opm, ser_opm};
