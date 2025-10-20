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
