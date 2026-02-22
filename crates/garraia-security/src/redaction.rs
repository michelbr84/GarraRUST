use tracing_subscriber::fmt::MakeWriter;

/// A writer that redacts sensitive tokens (API keys, bot tokens) from log output.
pub struct RedactingWriter<W> {
    inner: W,
}

impl RedactingWriter<std::io::Stderr> {
    pub fn stderr() -> Self {
        Self {
            inner: std::io::stderr(),
        }
    }
}

impl<W: std::io::Write> std::io::Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let original = String::from_utf8_lossy(buf);
        let redacted = redact_secrets(&original);
        self.inner.write_all(redacted.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<'a> MakeWriter<'a> for RedactingWriter<std::io::Stderr> {
    type Writer = RedactingWriter<std::io::Stderr>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter {
            inner: std::io::stderr(),
        }
    }
}

/// Replace known API key patterns with `[REDACTED]`.
pub fn redact_secrets(input: &str) -> String {
    // Patterns: Anthropic, OpenAI, Slack bot/app tokens, generic sk- prefixed keys
    static PATTERNS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?x)
              sk-ant-api\S{10,}    # Anthropic API keys
            | sk-\S{20,}           # OpenAI-style keys
            | xoxb-\S{10,}         # Slack bot tokens
            | xapp-\S{10,}         # Slack app tokens
            | xoxp-\S{10,}         # Slack user tokens
            | Bot\s+[A-Za-z0-9_\-]{30,}  # Discord bot tokens
            ",
        )
        .expect("redaction regex should compile")
    });

    PATTERNS.replace_all(input, "[REDACTED]").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_anthropic_key() {
        let input = "key=sk-ant-api03-abcdefghij";
        assert_eq!(redact_secrets(input), "key=[REDACTED]");
    }

    #[test]
    fn redacts_openai_key() {
        let input = "key=sk-1234567890123456789012345";
        assert_eq!(redact_secrets(input), "key=[REDACTED]");
    }

    #[test]
    fn redacts_slack_bot_token() {
        let input = "token=xoxb-1234567890-abc";
        assert_eq!(redact_secrets(input), "token=[REDACTED]");
    }

    #[test]
    fn leaves_normal_text_unchanged() {
        let input = "hello world";
        assert_eq!(redact_secrets(input), "hello world");
    }
}
