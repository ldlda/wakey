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

