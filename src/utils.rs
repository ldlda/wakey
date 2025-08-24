// pub const LDA_MACS: [[u8; 6]; 2] = [
//     // ether
//     [0x04, 0x7c, 0x16, 0x79, 0x6d, 0xee],
//     // wifi
//     [0xbc, 0x09, 0x1b, 0xec, 0x65, 0xd0],
// ];

/// is it time to lookup host lda.lan for this...
// pub static LDA_MACS_2: LazyLock<[MacAddr; 2]> = LazyLock::new(|| LDA_MACS.map(MacAddr::from));
pub mod wake;
use std::net::IpAddr;

pub mod error;

/// generic so you can do "123.45.67.89:22" or "lda.lan:22" as an input
// this is so bad
pub mod ping;

/// this is because i like [`IpAddr`] more than [`SocketAddr`](std::net::SocketAddr)
pub async fn get_ips(machine_name: &str) -> error::Result<Vec<IpAddr>> {
    let it = tokio::net::lookup_host((machine_name, 0))
        .await
        .map_err(|e| error::Error::DnsResolve {
            name: machine_name.to_string(),
            source: e,
        })?;
    Ok(it.map(|c| c.ip()).collect())
}

pub mod cmd;
pub mod query;

// no custom ip deserializer needed when using axum_extra::extract::Query
// but we add a generic one to ignore blanks and accept OneOrMany
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
