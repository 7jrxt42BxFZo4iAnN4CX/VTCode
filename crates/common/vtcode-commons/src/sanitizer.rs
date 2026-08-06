//! Secret sanitization utilities for redacting sensitive information.
//!
//! Provides regex-based secret redaction for:
//! - OpenAI API keys (`sk-...`)
//! - AWS Access Key IDs (`AKIA...`)
//! - Bearer tokens (`Bearer ...`)
//! - Generic secret assignments (`api_key=...`, `password:...`, etc.)
//!
//! Use this module to sanitize text before logging, displaying in UI,
//! or storing in session archives.

use regex::Regex;
use std::sync::LazyLock;

/// OpenAI API key pattern: sk- followed by alphanumeric characters
static OPENAI_KEY_REGEX: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"sk-[A-Za-z0-9]{20,}"));

/// AWS Access Key ID pattern: AKIA followed by 16 alphanumeric characters
static AWS_ACCESS_KEY_ID_REGEX: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"\bAKIA[0-9A-Z]{16}\b"));

/// Bearer token pattern: "Bearer " followed by token characters
static BEARER_TOKEN_REGEX: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?i)\bBearer\s+[A-Za-z0-9.\-_]{16,}\b"));

/// Generic secret assignment pattern: key=value or key: value format
/// Matches common secret key names like api_key, token, secret, password
static SECRET_ASSIGNMENT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r#"(?i)\b(api[\-_]?key|token|secret|password)\b(\s*[:=]\s*)(["']?)[^\s"']{8,}"#));

/// Redact secrets and sensitive keys from a string.
///
/// This is a best-effort operation using well-known regex patterns.
/// Redacted values are replaced with `[REDACTED_SECRET]`.
///
/// # Examples
///
/// ```
/// use vtcode_commons::sanitizer::redact_secrets;
///
/// let input = "Found key: sk-test1234567890abcdefghij".to_string();
/// let output = redact_secrets(input);
/// assert_eq!(output, "Found key: [REDACTED_SECRET]");
/// ```
pub fn redact_secrets(input: String) -> String {
    let r1 = OPENAI_KEY_REGEX.replace_all(&input, "[REDACTED_SECRET]");
    let r2 = AWS_ACCESS_KEY_ID_REGEX.replace_all(&r1, "[REDACTED_SECRET]");
    let r3 = BEARER_TOKEN_REGEX.replace_all(&r2, "Bearer [REDACTED_SECRET]");
    let r4 = SECRET_ASSIGNMENT_REGEX.replace_all(&r3, "$1$2$3[REDACTED_SECRET]");
    // `into_owned` clones only when the final result is `Borrowed` (no regex
    // matched at all); when any redaction occurred it moves the owned string
    // without an extra allocation. Do NOT short-circuit on `Cow::Borrowed` —
    // the final Cow is `Borrowed` whenever the *last* regex doesn't match,
    // even if earlier regexes did, which would silently discard redactions.
    r4.into_owned()
}

/// Incrementally redact streamed output without retaining the full stream.
///
/// A bounded suffix is held between chunks so a secret split at an IO
/// boundary is still matched by the same redaction rules as a complete line.
#[derive(Debug, Default)]
pub struct StreamingSecretRedactor {
    pending: String,
}

const STREAMING_REDACTION_CARRY_BYTES: usize = 1_024;

impl StreamingSecretRedactor {
    /// Redact and return the safe prefix of `chunk`. The returned string may
    /// be empty while the bounded carry window is being filled.
    pub fn push(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        if self.pending.len() <= STREAMING_REDACTION_CARRY_BYTES {
            if !self.pending.contains('\n') {
                return String::new();
            }
        }

        let carry_split = self.pending.len().saturating_sub(STREAMING_REDACTION_CARRY_BYTES);
        let line_split = self.pending.rfind('\n').map(|index| index + 1).unwrap_or(0);
        let mut split_at = carry_split.max(line_split);
        while split_at > 0 && !self.pending.is_char_boundary(split_at) {
            split_at -= 1;
        }
        let prefix: String = self.pending.drain(..split_at).collect();
        redact_secrets(prefix)
    }

    /// Redact and return the final carried suffix.
    pub fn finish(self) -> String {
        redact_secrets(self.pending)
    }
}

#[allow(clippy::panic)]
fn compile_regex(pattern: &str) -> Regex {
    match Regex::new(pattern) {
        Ok(regex) => regex,
        // Panic is acceptable thanks to the `load_regex` test
        Err(err) => panic!("invalid regex pattern `{pattern}`: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_regex() {
        // Verify all regex patterns compile without panicking
        let _ = redact_secrets("test".to_string());
    }

    #[test]
    fn redacts_openai_key() {
        let input = "Found key: sk-test1234567890abcdefghij".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "Found key: [REDACTED_SECRET]");
    }

    #[test]
    fn redacts_aws_access_key() {
        // AKIAIOSFODNN7EXAMPLE is AWS's well-known documentation example key.
        let input = " creds: AKIAIOSFODNN7EXAMPLE ".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, " creds: [REDACTED_SECRET] ");
    }

    #[test]
    fn redacts_bearer_token() {
        let input = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "Authorization: Bearer [REDACTED_SECRET]");
    }

    #[test]
    fn redacts_api_key_assignment() {
        let input = "api_key=sk-test12345678".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "api_key=[REDACTED_SECRET]");
    }

    #[test]
    fn redacts_password_assignment() {
        let input = "password: mysecretvalue".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "password: [REDACTED_SECRET]");
    }

    #[test]
    fn redacts_token_in_quotes() {
        let input = r#"token="abc123xyz789abcdef""#.to_string();
        let output = redact_secrets(input);
        assert_eq!(output, r#"token="[REDACTED_SECRET]""#);
    }

    #[test]
    fn preserves_short_values() {
        // Values under 8 characters should not be redacted
        let input = "password: short".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "password: short");
    }

    #[test]
    fn redacts_multiple_secrets() {
        let input = "Keys: sk-test1234567890abcdefghij and AKIAIOSFODNN7EXAMPLE".to_string();
        let output = redact_secrets(input);
        // Verify both secrets are redacted
        assert!(output.contains("[REDACTED_SECRET]"));
        assert!(!output.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!output.contains("sk-test1234567890abcdefghij"));
    }

    #[test]
    fn preserves_non_secret_text() {
        let input = "Hello world, this is normal text".to_string();
        let output = redact_secrets(input);
        assert_eq!(output, "Hello world, this is normal text");
    }

    #[test]
    fn redacts_secrets_split_across_stream_chunks() {
        let mut redactor = StreamingSecretRedactor::default();
        let mut output = redactor.push("password=superse");
        output.push_str(&redactor.push("cretvalue\n"));
        output.push_str(&redactor.finish());

        assert_eq!(output, "password=[REDACTED_SECRET]\n");
        assert!(!output.contains("supersecretvalue"));
    }
}
