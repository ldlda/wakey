/// Parses Chrome-style numeric IPv4 forms: hex (0x...), decimal, octal.
pub fn parse_numeric_ipv4(s: &str) -> Option<std::net::IpAddr> {
    let s = s.trim();
    // hex
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
        && hex.chars().all(|c| c.is_ascii_hexdigit())
        && let Ok(n) = u32::from_str_radix(hex, 16)
    {
        return Some(std::net::IpAddr::V4(std::net::Ipv4Addr::from(n)));
    }
    // decimal
    if s.chars().all(|c| c.is_ascii_digit())
        && let Ok(n) = s.parse::<u32>()
    {
        return Some(std::net::IpAddr::V4(std::net::Ipv4Addr::from(n)));
    }
    // octal (leading 0, all octal digits)
    if s.len() > 1
        && s.as_bytes()[0] == b'0'
        && s.chars().all(|c| matches!(c, '0'..='7'))
        && let Ok(n) = u32::from_str_radix(s, 8)
    {
        return Some(std::net::IpAddr::V4(std::net::Ipv4Addr::from(n)));
    }
    None
}
/// Extracts the host portion from a URL-like string, for smart input parsing.
pub fn extract_host(input: &str) -> &str {
    let mut s = input.trim();
    // Strip scheme (e.g., http://, https://, ssh://) or network-path reference (//host)
    if let Some(idx) = s.find("://") {
        s = &s[idx + 3..];
    } else if let Some(rest) = s.strip_prefix("//") {
        s = rest;
    }
    // Strip potential userinfo (user@host)
    if let Some((_, host)) = s.rsplit_once('@') {
        s = host;
    }
    // If bracketed IPv6 like [::1]:8080/path -> extract inside brackets
    if let Some(host) = s.strip_prefix('[') {
        if let Some(end) = host.find(']') {
            s = &host[..end];
        }
    } else {
        // Trim path suffix if any
        if let Some(pos) = s.find('/') {
            s = &s[..pos];
        }
        // Drop trailing :port if present and numeric, but only if there's exactly one ':'
        if let Some((host, port)) = s.rsplit_once(':')
            && s.matches(':').count() == 1
            && port.chars().all(|c| c.is_ascii_digit())
        {
            s = host;
        }
    }
    s.trim()
}

/// key for yes: "1" | "true" | "yes" | "on" | "y"
///
/// frfr
pub fn _de_boolish<'de, D>(des: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Boolish {
        B(bool),
        I(u8),
        S(String),
    }
    Ok(match Boolish::deserialize(des)? {
        Boolish::B(b) => b,
        Boolish::I(i) => i != 0,
        Boolish::S(s) => {
            let t = s.trim().to_ascii_lowercase();
            if t.is_empty() {
                true // presence implies true
            } else {
                matches!(t.as_str(), "1" | "true" | "yes" | "on" | "y")
            }
        }
    })
}

/// Parse a tolerant boolean value from a string.
/// Accepts: "1", "true", "yes", "on", "y" as true; "0", "false", "no", "off", "n" as false.
/// Empty string means true (presence-only query flag).
pub fn boolish_str(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return true;
    }
    matches!(t.as_str(), "1" | "true" | "yes" | "on" | "y")
        || (!matches!(t.as_str(), "0" | "false" | "no" | "off" | "n")
            && t.parse::<u64>().map(|n| n != 0).unwrap_or(false))
}

pub mod de_many {
    use serde::Deserialize;
    use serde::de;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        One(T),
        Many(Vec<T>),
    }

    pub fn vec_from_strs<'de, D, T>(des: D) -> Result<Vec<T>, D::Error>
    where
        D: serde::Deserializer<'de>,
        T: std::str::FromStr,
        T::Err: std::fmt::Display,
    {
        let raw: OneOrMany<String> = OneOrMany::<String>::deserialize(des)?;
        let mut out = Vec::new();
        match raw {
            OneOrMany::One(s) => {
                let t = s.trim();
                if !t.is_empty() {
                    out.push(t.parse().map_err(de::Error::custom)?);
                }
            }
            OneOrMany::Many(vs) => {
                for s in vs {
                    let t = s.trim();
                    if t.is_empty() {
                        continue;
                    }
                    out.push(t.parse().map_err(de::Error::custom)?);
                }
            }
        }
        Ok(out)
    }
}

pub mod mac {
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
    pub fn _serialize_mac<S>(mac: &MacAddr, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&mac.to_string())
    }

    /// Deserialize a MacAddr from a string
    pub fn _deserialize_mac<'de, D>(deserializer: D) -> Result<MacAddr, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = <String as serde::Deserialize>::deserialize(deserializer)?;
        s.parse::<MacAddr>().map_err(DeError::custom)
    }

    /// serialize an [`Option<MacAddr>`]
    pub fn ser_opm<S: Serializer>(bro: &Option<MacAddr>, ser: S) -> Result<S::Ok, S::Error> {
        Option::<String>::serialize(&bro.as_ref().map(ToString::to_string), ser)
    }

    /// deserialize an [`Option<MacAddr>`]
    pub fn des_opm<'de, D>(des: D) -> Result<Option<MacAddr>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Option::<&str>::deserialize(des)?
            .map(str::parse)
            .transpose()
            .map_err(de::Error::custom)
    }
}

pub use mac::*;
