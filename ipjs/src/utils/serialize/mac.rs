use macaddr::MacAddr;
use serde::{self, Deserialize, Deserializer, de::Error as DeError};
use serde::{Serialize, Serializer, de};

pub fn serialize_macs<S>(macs: &[MacAddr], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let strings: Vec<String> = macs.iter().map(|m| m.to_string()).collect();
    serde::Serialize::serialize(&strings, serializer)
}

/// Serialize a MacAddr as a string
pub fn serialize<S>(mac: &MacAddr, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&mac.to_string())
}

/// Deserialize a MacAddr from a string
pub fn deserialize<'de, D>(deserializer: D) -> Result<MacAddr, D::Error>
where
    D: Deserializer<'de>,
{
    let s = <String as serde::Deserialize>::deserialize(deserializer)?;
    s.parse::<MacAddr>().map_err(DeError::custom)
}

pub mod option_mac {
    use super::*;
    /// serialize an [`Option<MacAddr>`]
    pub fn serialize<S: Serializer>(bro: &Option<MacAddr>, ser: S) -> Result<S::Ok, S::Error> {
        Option::<String>::serialize(&bro.as_ref().map(ToString::to_string), ser)
    }
    /// deserialize an [`Option<MacAddr>`]
    pub fn deserialize<'de, D>(des: D) -> Result<Option<MacAddr>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::<&str>::deserialize(des)?
            .map(str::parse)
            .transpose()
            .map_err(de::Error::custom)
    }
}
