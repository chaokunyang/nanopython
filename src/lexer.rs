use crate::token::{keyword, Token, TokenKind};

type LexResult<T> = Result<T, String>;

pub fn lex(source: &str) -> LexResult<Vec<Token>> {
    let source = strip_triple_quoted(source);
    let mut tokens = Vec::new();
    let mut indents = vec![0usize];
    let mut paren_depth = 0isize;

    for (line_no, raw_line) in source.lines().enumerate() {
        let line_idx = line_no + 1;
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut indent = 0usize;
        for ch in line.chars() {
            if ch == ' ' {
                indent += 1;
            } else if ch == '\t' {
                return Err(format!(
                    "tabs are not supported for indentation at line {line_idx}"
                ));
            } else {
                break;
            }
        }

        if paren_depth == 0 {
            let current = *indents.last().expect("indent stack must not be empty");
            if indent > current {
                indents.push(indent);
                tokens.push(Token {
                    kind: TokenKind::Indent,
                    line: line_idx,
                    col: 1,
                });
            } else if indent < current {
                while indent < *indents.last().expect("indent stack must not be empty") {
                    indents.pop();
                    tokens.push(Token {
                        kind: TokenKind::Dedent,
                        line: line_idx,
                        col: 1,
                    });
                }
                if indent != *indents.last().expect("indent stack must not be empty") {
                    return Err(format!("inconsistent indentation at line {line_idx}"));
                }
            }
        }

        let before = tokens.len();
        tokenize_line(&line[indent..], line_idx, indent + 1, &mut tokens)?;
        for token in &tokens[before..] {
            match token.kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => paren_depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    if paren_depth > 0 {
                        paren_depth -= 1;
                    }
                }
                _ => {}
            }
        }
        if paren_depth == 0 {
            tokens.push(Token {
                kind: TokenKind::Newline,
                line: line_idx,
                col: line.len().saturating_add(1),
            });
        }
    }

    while indents.len() > 1 {
        indents.pop();
        tokens.push(Token {
            kind: TokenKind::Dedent,
            line: source.lines().count().saturating_add(1),
            col: 1,
        });
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        line: source.lines().count().saturating_add(1),
        col: 1,
    });
    Ok(tokens)
}

fn strip_triple_quoted(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut i = 0usize;
    while i < chars.len() {
        if i + 2 < chars.len()
            && ((chars[i] == '"' && chars[i + 1] == '"' && chars[i + 2] == '"')
                || (chars[i] == '\'' && chars[i + 1] == '\'' && chars[i + 2] == '\''))
        {
            let quote = chars[i];
            // Keep a parseable placeholder token so docstring-only blocks are not empty.
            out.push(quote);
            out.push(quote);
            i += 3;
            while i + 2 < chars.len()
                && !(chars[i] == quote && chars[i + 1] == quote && chars[i + 2] == quote)
            {
                if chars[i] == '\n' {
                    out.push('\n');
                } else {
                    out.push(' ');
                }
                i += 1;
            }
            if i + 2 < chars.len() {
                i += 3;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn tokenize_line(
    line: &str,
    line_no: usize,
    col_offset: usize,
    out: &mut Vec<Token>,
) -> LexResult<()> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '#' {
            break;
        }
        if ch.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let col = col_offset + i;
        if let Some(prefix_len) = string_prefix_len(&chars, i) {
            let is_fstring = prefix_has_f(&chars, i, prefix_len);
            i += prefix_len;
            let value = parse_string_literal(&chars, &mut i, line_no, col)?;
            out.push(Token {
                kind: if is_fstring {
                    TokenKind::FStr(value)
                } else {
                    TokenKind::Str(value)
                },
                line: line_no,
                col,
            });
            continue;
        }

        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            let kind = if let Some(kw) = keyword(&name) {
                TokenKind::Keyword(kw)
            } else {
                TokenKind::Name(name)
            };
            out.push(Token {
                kind,
                line: line_no,
                col,
            });
            continue;
        }

        if ch.is_ascii_digit() {
            let start = i;
            i += 1;
            if chars[start] == '0' && i < chars.len() && matches!(chars[i], 'x' | 'X') {
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_hexdigit() || chars[i] == '_')
                {
                    i += 1;
                }
            } else {
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                    i += 1;
                }
            }
            let raw: String = chars[start..i].iter().collect();
            let value = parse_int_literal(&raw)
                .map_err(|_| format!("invalid integer literal '{raw}' at line {line_no}:{col}"))?;
            out.push(Token {
                kind: TokenKind::Int(value),
                line: line_no,
                col,
            });
            continue;
        }

        if ch == '\'' || ch == '"' {
            let value = parse_string_literal(&chars, &mut i, line_no, col)?;
            out.push(Token {
                kind: TokenKind::Str(value),
                line: line_no,
                col,
            });
            continue;
        }

        let three = if i + 2 < chars.len() {
            Some((ch, chars[i + 1], chars[i + 2]))
        } else {
            None
        };
        let matched_three = match three {
            Some(('<', '<', '=')) => Some(TokenKind::LShiftEq),
            Some(('>', '>', '=')) => Some(TokenKind::RShiftEq),
            Some(('/', '/', '=')) => Some(TokenKind::SlashSlashEq),
            _ => None,
        };
        if let Some(kind) = matched_three {
            out.push(Token {
                kind,
                line: line_no,
                col,
            });
            i += 3;
            continue;
        }

        let two = if i + 1 < chars.len() {
            Some((ch, chars[i + 1]))
        } else {
            None
        };

        let matched_two = match two {
            Some(('=', '=')) => Some(TokenKind::EqEq),
            Some(('!', '=')) => Some(TokenKind::NotEq),
            Some(('<', '=')) => Some(TokenKind::Lte),
            Some(('>', '=')) => Some(TokenKind::Gte),
            Some(('+', '=')) => Some(TokenKind::PlusEq),
            Some(('-', '=')) => Some(TokenKind::MinusEq),
            Some(('*', '=')) => Some(TokenKind::StarEq),
            Some(('/', '=')) => Some(TokenKind::SlashEq),
            Some(('/', '/')) => Some(TokenKind::SlashSlash),
            Some(('%', '=')) => Some(TokenKind::PercentEq),
            Some(('&', '=')) => Some(TokenKind::AmpEq),
            Some(('|', '=')) => Some(TokenKind::PipeEq),
            Some(('^', '=')) => Some(TokenKind::CaretEq),
            Some(('<', '<')) => Some(TokenKind::LShift),
            Some(('>', '>')) => Some(TokenKind::RShift),
            _ => None,
        };
        if let Some(kind) = matched_two {
            out.push(Token {
                kind,
                line: line_no,
                col,
            });
            i += 2;
            continue;
        }

        let kind = match ch {
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            ',' => TokenKind::Comma,
            ':' => TokenKind::Colon,
            '.' => TokenKind::Dot,
            '@' => TokenKind::At,
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            '&' => TokenKind::Amp,
            '|' => TokenKind::Pipe,
            '^' => TokenKind::Caret,
            '=' => TokenKind::Eq,
            '<' => TokenKind::Lt,
            '>' => TokenKind::Gt,
            _ => {
                return Err(format!(
                    "unsupported character '{ch}' at line {line_no}:{col}"
                ));
            }
        };
        out.push(Token {
            kind,
            line: line_no,
            col,
        });
        i += 1;
    }
    Ok(())
}

fn string_prefix_len(chars: &[char], i: usize) -> Option<usize> {
    if i >= chars.len() {
        return None;
    }
    let is_prefix = |c: char| matches!(c, 'f' | 'F' | 'r' | 'R' | 'b' | 'B' | 'u' | 'U');
    if i + 1 < chars.len() && is_prefix(chars[i]) && (chars[i + 1] == '\'' || chars[i + 1] == '"') {
        return Some(1);
    }
    if i + 2 < chars.len()
        && is_prefix(chars[i])
        && is_prefix(chars[i + 1])
        && (chars[i + 2] == '\'' || chars[i + 2] == '"')
    {
        return Some(2);
    }
    None
}

fn prefix_has_f(chars: &[char], i: usize, len: usize) -> bool {
    (0..len).any(|idx| matches!(chars.get(i + idx), Some('f' | 'F')))
}

fn parse_string_literal(
    chars: &[char],
    i: &mut usize,
    line_no: usize,
    col: usize,
) -> LexResult<String> {
    if *i >= chars.len() || (chars[*i] != '\'' && chars[*i] != '"') {
        return Err(format!("invalid string literal at line {line_no}:{col}"));
    }
    let quote = chars[*i];
    *i += 1;
    let mut value = String::new();
    while *i < chars.len() {
        let c = chars[*i];
        if c == '\\' {
            *i += 1;
            if *i >= chars.len() {
                return Err(format!("invalid escape at line {line_no}:{col}"));
            }
            let esc = chars[*i];
            value.push(match esc {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                other => other,
            });
            *i += 1;
            continue;
        }
        if c == quote {
            *i += 1;
            break;
        }
        value.push(c);
        *i += 1;
    }
    Ok(value)
}

fn parse_int_literal(raw: &str) -> Result<i64, std::num::ParseIntError> {
    let cleaned = raw.replace('_', "");
    if cleaned.starts_with("0x") || cleaned.starts_with("0X") {
        i64::from_str_radix(&cleaned[2..], 16)
    } else {
        cleaned.parse::<i64>()
    }
}
