/// An opaque Wakey bearer token. Its value is only exposed by explicit access
/// or formatting for transport; this type deliberately has no Debug or Display.
pub struct BearerToken<'a>(&'a str);

impl<'a> BearerToken<'a> {
    /// Accepts a nonempty token with no whitespace or control bytes.
    pub fn new(token: &'a str) -> Option<Self> {
        (!token.is_empty() && token.bytes().all(|byte| byte.is_ascii_graphic()))
            .then_some(Self(token))
    }

    /// Parses an Authorization value with the case-insensitive Bearer scheme.
    pub fn parse_authorization(value: &'a str) -> Option<Self> {
        let (scheme, token) = value.split_once(' ')?;
        if !scheme.eq_ignore_ascii_case("Bearer") {
            return None;
        }
        Self::new(token)
    }

    pub fn as_str(&self) -> &'a str {
        self.0
    }

    /// Produces a sensitive Authorization value. Do not log the returned string.
    pub fn to_authorization(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::BearerToken;

    #[test]
    fn bearer_round_trips_and_rejects_invalid_tokens_on_both_paths() {
        let header = BearerToken::new("token-123").unwrap().to_authorization();
        assert_eq!(header, "Bearer token-123");
        assert_eq!(
            BearerToken::parse_authorization(&header).unwrap().as_str(),
            "token-123"
        );
        assert!(BearerToken::parse_authorization("Basic token-123").is_none());
        for token in [
            "",
            "two words",
            " leading",
            "trailing ",
            "bad\r\nheader",
            "caf\u{e9}",
        ] {
            assert!(BearerToken::new(token).is_none());
            assert!(BearerToken::parse_authorization(&format!("Bearer {token}")).is_none());
        }
    }
}
