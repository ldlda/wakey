//! Human-readable config durations and the unit-aware legacy key adapter.
//! Protocol numbers and metric units deliberately do not use this adapter.

use std::time::Duration;

use serde::{Deserialize, Deserializer, de::Error};
use serde_json::Value;

#[derive(Clone, Copy)]
pub struct LegacyDuration {
    pub old: &'static str,
    pub new: &'static str,
    pub unit: Duration,
}

/// Normalize one config table before normal serde decoding. The returned keys
/// let loaders warn only after validation, even before tracing is initialized.
pub fn normalize(
    value: &mut Value,
    keys: &[LegacyDuration],
) -> Result<Vec<LegacyDuration>, String> {
    let table = value.as_object_mut().ok_or("expected a config table")?;
    let mut deprecated = Vec::new();
    for key in keys {
        if table.contains_key(key.old) && table.contains_key(key.new) {
            return Err(format!(
                "configure only one of `{}` and `{}`",
                key.old, key.new
            ));
        }
        if let Some(old) = table.remove(key.old) {
            let duration = if let Some(number) = old.as_u64() {
                // u64 multiplication must not wrap for large legacy day values.
                let nanos = key
                    .unit
                    .as_nanos()
                    .checked_mul(u128::from(number))
                    .ok_or_else(|| format!("`{}` is too large", key.old))?;
                let seconds = u64::try_from(nanos / 1_000_000_000)
                    .map_err(|_| format!("`{}` is too large", key.old))?;
                Duration::new(seconds, (nanos % 1_000_000_000) as u32)
            } else if let Some(text) = old.as_str() {
                humantime::parse_duration(text).map_err(|err| format!("`{}`: {err}", key.old))?
            } else {
                return Err(format!(
                    "`{}` requires a non-negative integer or duration string",
                    key.old
                ));
            };
            table.insert(
                key.new.into(),
                Value::String(humantime::format_duration(duration).to_string()),
            );
            deprecated.push(*key);
        }
        if let Some(value) = table.get(key.new) {
            let text = value
                .as_str()
                .ok_or_else(|| format!("`{}` requires a duration string with units", key.new))?;
            humantime::parse_duration(text).map_err(|err| format!("`{}`: {err}", key.new))?;
        }
    }
    Ok(deprecated)
}

/// Decode through the canonical serde definition after translating old keys.
/// The callback is a serde-generated associated function (`remote = "Self"`),
/// avoiding a second copy of every unrelated config field in a legacy DTO.
pub fn deserialize_legacy<'de, D, T>(
    deserializer: D,
    keys: &[LegacyDuration],
    decode: impl FnOnce(Value) -> Result<T, serde_json::Error>,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
{
    let mut value = Value::deserialize(deserializer)?;
    normalize(&mut value, keys).map_err(D::Error::custom)?;
    decode(value).map_err(D::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEYS: &[LegacyDuration] = &[LegacyDuration {
        old: "interval_ms",
        new: "interval",
        unit: Duration::from_millis(1),
    }];

    #[test]
    fn legacy_units_are_exact_and_conflicts_are_rejected() {
        let mut value = serde_json::json!({"interval_ms": 1234});
        assert_eq!(normalize(&mut value, KEYS).unwrap().len(), 1);
        assert_eq!(
            humantime::parse_duration(value["interval"].as_str().unwrap()).unwrap(),
            Duration::from_millis(1234)
        );
        assert!(
            normalize(
                &mut serde_json::json!({"interval_ms": 1, "interval": "1s"}),
                KEYS
            )
            .is_err()
        );
        for invalid in [
            serde_json::json!({"interval": 5}),
            serde_json::json!({"interval_ms": -1}),
            serde_json::json!({"interval": "potato"}),
        ] {
            assert!(normalize(&mut invalid.clone(), KEYS).is_err());
        }
    }
}
