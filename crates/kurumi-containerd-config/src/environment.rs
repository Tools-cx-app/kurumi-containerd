use std::collections::BTreeMap;

use kurumi_containerd_error::{config::ErrorContext as _, config_ensure as ensure};

use crate::Result;

#[derive(Debug, PartialEq, Eq)]
enum Token<'a> {
    Export,
    Key(&'a str),
    Equals,
    Value(&'a str),
}

/// Each invocation is one logical line; values consume everything after `=`.
fn lex_line(line: &str) -> impl Iterator<Item = Token<'_>> {
    let line = line.trim();
    let mut tokens = [None, None, None, None];
    if !line.is_empty() && !line.starts_with('#') {
        let line = if let Some(rest) = line.strip_prefix("export ") {
            tokens[0] = Some(Token::Export);
            rest.trim_start()
        } else {
            line
        };
        if let Some((key, raw_value)) = line.split_once('=') {
            tokens[1] = Some(Token::Key(key));
            tokens[2] = Some(Token::Equals);
            let value = ['\'', '"']
                .into_iter()
                .find_map(|quote| raw_value.strip_prefix(quote)?.strip_suffix(quote))
                .unwrap_or(raw_value);
            tokens[3] = Some(Token::Value(value));
        } else {
            tokens[1] = Some(Token::Key(line));
        }
    }
    tokens.into_iter().flatten()
}

/// Parses newline-separated `KEY=VALUE` environment entries.
///
/// # Errors
///
/// Returns an error for malformed lines, invalid variable names, or NUL bytes.
pub fn parse_environment(source: &str) -> Result<BTreeMap<String, String>> {
    let mut environment = BTreeMap::new();
    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let mut tokens = lex_line(line).peekable();
        if tokens.peek() == Some(&Token::Export) {
            tokens.next();
        }
        let Some(Token::Key(key)) = tokens.next() else {
            continue;
        };
        ensure!(
            tokens.next() == Some(Token::Equals),
            "line {line_number} has no '='"
        );
        ensure!(valid_env_key(key), "invalid key on line {line_number}");
        let value = match tokens.next() {
            Some(Token::Value(value)) => Some(value),
            _ => None,
        }
        .with_context(|| format!("missing value on line {line_number}"))?;
        ensure!(!value.contains('\0'), "NUL byte on line {line_number}");
        environment.insert(key.to_owned(), value.to_owned());
    }
    Ok(environment)
}

pub(crate) fn valid_env_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::{Token, lex_line};

    #[test]
    fn tokenizes_assignment_boundaries() {
        assert_eq!(
            lex_line("export KEY='a=b # literal'").collect::<Vec<_>>(),
            [
                Token::Export,
                Token::Key("KEY"),
                Token::Equals,
                Token::Value("a=b # literal"),
            ]
        );
        assert_eq!(
            lex_line("EMPTY=").collect::<Vec<_>>(),
            [Token::Key("EMPTY"), Token::Equals, Token::Value("")]
        );
        assert_eq!(
            lex_line("MISSING").collect::<Vec<_>>(),
            [Token::Key("MISSING")]
        );
        assert!(lex_line("  # comment").next().is_none());
        assert!(lex_line("  ").next().is_none());
    }
}
