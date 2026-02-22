/// Convert standard markdown to Telegram MarkdownV2 format.
///
/// Telegram MarkdownV2 requires escaping these characters outside of
/// formatting entities: `_`, `*`, `[`, `]`, `(`, `)`, `~`, `` ` ``, `>`,
/// `#`, `+`, `-`, `=`, `|`, `{`, `}`, `.`, `!`
///
/// This function handles:
/// - Code blocks (``` ... ```) — preserved as-is
/// - Inline code (` ... `) — preserved as-is
/// - Bold (**text**) → *text*
/// - Italic (*text* or _text_) → _text_
/// - Escaping special chars in plain text
pub fn to_telegram_markdown(input: &str) -> String {
    let mut result = String::with_capacity(input.len() * 2);
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Fenced code block: ```...```
        if i + 2 < len && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
            // Find the language tag (rest of line after ```)
            let block_start = i;
            i += 3;

            // Skip optional language tag
            while i < len && chars[i] != '\n' {
                i += 1;
            }

            let header = &input[byte_offset(&chars, block_start)..byte_offset(&chars, i)];

            // Find closing ```
            let content_start = i;
            let mut found_close = false;
            while i < len {
                if i + 2 < len && chars[i] == '`' && chars[i + 1] == '`' && chars[i + 2] == '`' {
                    let content =
                        &input[byte_offset(&chars, content_start)..byte_offset(&chars, i)];
                    result.push_str(header);
                    result.push_str(content);
                    result.push_str("```");
                    i += 3;
                    found_close = true;
                    break;
                }
                i += 1;
            }
            if !found_close {
                // Unclosed code block — just push the rest
                let content = &input[byte_offset(&chars, content_start)..byte_offset(&chars, len)];
                result.push_str(header);
                result.push_str(content);
            }
            continue;
        }

        // Inline code: `...`
        if chars[i] == '`' {
            result.push('`');
            i += 1;
            while i < len && chars[i] != '`' {
                result.push(chars[i]);
                i += 1;
            }
            if i < len {
                result.push('`');
                i += 1;
            }
            continue;
        }

        // Bold: **text** → *text*
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            result.push('*');
            i += 2;
            while i < len && !(i + 1 < len && chars[i] == '*' && chars[i + 1] == '*') {
                if is_special(chars[i]) && chars[i] != '*' {
                    result.push('\\');
                }
                result.push(chars[i]);
                i += 1;
            }
            result.push('*');
            if i + 1 < len {
                i += 2; // skip closing **
            }
            continue;
        }

        // Escape special characters in plain text
        if is_special(chars[i]) {
            result.push('\\');
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

fn is_special(c: char) -> bool {
    matches!(
        c,
        '_' | '*'
            | '['
            | ']'
            | '('
            | ')'
            | '~'
            | '`'
            | '>'
            | '#'
            | '+'
            | '-'
            | '='
            | '|'
            | '{'
            | '}'
            | '.'
            | '!'
    )
}

fn byte_offset(chars: &[char], char_index: usize) -> usize {
    chars[..char_index].iter().map(|c| c.len_utf8()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_escapes_special_chars() {
        assert_eq!(to_telegram_markdown("hello.world"), "hello\\.world");
        assert_eq!(to_telegram_markdown("1 + 2 = 3"), "1 \\+ 2 \\= 3");
        assert_eq!(to_telegram_markdown("no specials"), "no specials");
    }

    #[test]
    fn code_blocks_preserved() {
        let input = "```rust\nfn main() {}\n```";
        let output = to_telegram_markdown(input);
        assert_eq!(output, "```rust\nfn main() {}\n```");
    }

    #[test]
    fn inline_code_preserved() {
        let input = "use `foo.bar()` here";
        let output = to_telegram_markdown(input);
        assert_eq!(output, "use `foo.bar()` here");
    }

    #[test]
    fn bold_converted() {
        let input = "this is **bold** text";
        let output = to_telegram_markdown(input);
        assert_eq!(output, "this is *bold* text");
    }

    #[test]
    fn mixed_formatting() {
        let input = "Hello! Try `code` and **bold**.";
        let output = to_telegram_markdown(input);
        assert_eq!(output, "Hello\\! Try `code` and *bold*\\.");
    }
}
