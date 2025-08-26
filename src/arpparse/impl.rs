//! r#impl AHHHHHH

use macaddr::MacAddr;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

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

/// serialize an [`Option<MacAddr>`]
pub fn ser_opm<S: Serializer>(bro: &Option<MacAddr>, ser: S) -> Result<S::Ok, S::Error> {
    Option::<String>::serialize(&bro.as_ref().map(ToString::to_string), ser)
}

/// deserialize an [`Option<MacAddr>`]
pub fn _des_opm<'de, D>(des: D) -> Result<Option<MacAddr>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<&str>::deserialize(des)?
        .map(str::parse)
        .transpose()
        .map_err(de::Error::custom)
}
