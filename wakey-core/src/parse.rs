pub fn parse_numeric_ipv4(s: &str) -> Option<std::net::IpAddr> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
        && hex.chars().all(|c| c.is_ascii_hexdigit())
        && let Ok(n) = u32::from_str_radix(hex, 16)
    {
        return Some(std::net::IpAddr::V4(std::net::Ipv4Addr::from(n)));
    }
    if s.chars().all(|c| c.is_ascii_digit())
        && let Ok(n) = s.parse::<u32>()
    {
        return Some(std::net::IpAddr::V4(std::net::Ipv4Addr::from(n)));
    }
    if s.len() > 1
        && s.as_bytes()[0] == b'0'
        && s.chars().all(|c| matches!(c, '0'..='7'))
        && let Ok(n) = u32::from_str_radix(s, 8)
    {
        return Some(std::net::IpAddr::V4(std::net::Ipv4Addr::from(n)));
    }
    None
}

pub fn extract_host(input: &str) -> &str {
    let mut s = input.trim();
    if let Some(idx) = s.find("://") {
        s = &s[idx + 3..];
    } else if let Some(rest) = s.strip_prefix("//") {
        s = rest;
    }
    if let Some((_, host)) = s.rsplit_once('@') {
        s = host;
    }
    if let Some(host) = s.strip_prefix('[') {
        if let Some(end) = host.find(']') {
            s = &host[..end];
        }
    } else {
        if let Some(pos) = s.find('/') {
            s = &s[..pos];
        }
        if let Some((host, port)) = s.rsplit_once(':')
            && s.matches(':').count() == 1
            && port.chars().all(|c| c.is_ascii_digit())
        {
            s = host;
        }
    }
    s.trim()
}

pub fn boolish_str(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return true;
    }
    matches!(t.as_str(), "1" | "true" | "yes" | "on" | "y")
        || (!matches!(t.as_str(), "0" | "false" | "no" | "off" | "n")
            && t.parse::<u64>().map(|n| n != 0).unwrap_or(false))
}

pub mod mac {
    use macaddr::MacAddr;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(m: &MacAddr, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&m.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<MacAddr, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }

    pub mod option_mac {
        use macaddr::MacAddr;
        use serde::{Deserialize, Deserializer, Serializer};

        pub fn serialize<S>(m: &Option<MacAddr>, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match m {
                Some(mac) => serializer.serialize_some(&mac.to_string()),
                None => serializer.serialize_none(),
            }
        }

        pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<MacAddr>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = Option::<String>::deserialize(deserializer)?;
            s.map(|x| x.parse())
                .transpose()
                .map_err(serde::de::Error::custom)
        }
    }
}
