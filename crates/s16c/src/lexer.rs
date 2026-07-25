//! Scanner: source text -> tokens with 1-indexed positions (LANGUAGE.md §1).

use crate::{CompileError, Pos};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    // keywords
    Fn,
    Var,
    Static,
    Const,
    Use,
    If,
    Else,
    While,
    Loop,
    Break,
    Continue,
    Return,
    True,
    False,
    As,
    U16,
    I16,
    Bool,
    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    Arrow, // ->
    // operators
    Assign, // =
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Bang,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub pos: Pos,
}

pub fn lex(source: &str) -> Result<Vec<Token>, CompileError> {
    let mut tokens = Vec::new();
    let mut chars = source.chars().peekable();
    let mut line = 1usize;
    let mut col = 1usize;

    macro_rules! bump {
        () => {{
            let c = chars.next();
            if c == Some('\n') {
                line += 1;
                col = 1;
            } else if c.is_some() {
                col += 1;
            }
            c
        }};
    }

    loop {
        // skip whitespace and // comments
        loop {
            match chars.peek() {
                Some(c) if c.is_whitespace() => {
                    bump!();
                }
                Some('/') => {
                    let mut ahead = chars.clone();
                    ahead.next();
                    if ahead.peek() == Some(&'/') {
                        while chars.peek().is_some() && *chars.peek().unwrap() != '\n' {
                            bump!();
                        }
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }

        let pos = Pos { line, col };
        let Some(&c) = chars.peek() else { break };
        let err = |msg: String| CompileError::at(pos, msg);

        let tok = if c.is_ascii_alphabetic() || c == '_' {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() || c == '_' {
                    word.push(c);
                    bump!();
                } else {
                    break;
                }
            }
            keyword(&word).unwrap_or(Tok::Ident(word))
        } else if c.is_ascii_digit() {
            let mut digits = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() {
                    digits.push(c);
                    bump!();
                } else {
                    break;
                }
            }
            let value = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
                i64::from_str_radix(hex, 16).ok()
            } else {
                digits.parse::<i64>().ok()
            };
            Tok::Int(value.ok_or_else(|| err(format!("bad integer literal `{digits}`")))?)
        } else if c == '\'' {
            bump!();
            let ch = match bump!() {
                Some('\\') => match bump!() {
                    Some('n') => b'\n',
                    Some('t') => b'\t',
                    Some('r') => b'\r',
                    Some('0') => 0,
                    Some('\\') => b'\\',
                    Some('\'') => b'\'',
                    Some('"') => b'"',
                    other => return Err(err(format!("bad escape `\\{}`", other.unwrap_or(' ')))),
                },
                Some(c) if c.is_ascii() && c != '\'' => c as u8,
                _ => return Err(err("bad character literal".into())),
            };
            if bump!() != Some('\'') {
                return Err(err("unterminated character literal".into()));
            }
            Tok::Int(ch as i64)
        } else {
            bump!();
            let two = |chars: &mut std::iter::Peekable<std::str::Chars>, want: char| {
                if chars.peek() == Some(&want) {
                    chars.next();
                    true
                } else {
                    false
                }
            };
            // note: `two` consumes without the macro; fix col manually
            let mut took_second = false;
            let tok = match c {
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                '{' => Tok::LBrace,
                '}' => Tok::RBrace,
                '[' => Tok::LBracket,
                ']' => Tok::RBracket,
                ',' => Tok::Comma,
                ';' => Tok::Semi,
                ':' => Tok::Colon,
                '+' => Tok::Plus,
                '*' => Tok::Star,
                '/' => Tok::Slash,
                '%' => Tok::Percent,
                '^' => Tok::Caret,
                '-' => {
                    if two(&mut chars, '>') {
                        took_second = true;
                        Tok::Arrow
                    } else {
                        Tok::Minus
                    }
                }
                '=' => {
                    if two(&mut chars, '=') {
                        took_second = true;
                        Tok::Eq
                    } else {
                        Tok::Assign
                    }
                }
                '!' => {
                    if two(&mut chars, '=') {
                        took_second = true;
                        Tok::Ne
                    } else {
                        Tok::Bang
                    }
                }
                '<' => {
                    if two(&mut chars, '<') {
                        took_second = true;
                        Tok::Shl
                    } else if two(&mut chars, '=') {
                        took_second = true;
                        Tok::Le
                    } else {
                        Tok::Lt
                    }
                }
                '>' => {
                    if two(&mut chars, '>') {
                        took_second = true;
                        Tok::Shr
                    } else if two(&mut chars, '=') {
                        took_second = true;
                        Tok::Ge
                    } else {
                        Tok::Gt
                    }
                }
                '&' => {
                    if two(&mut chars, '&') {
                        took_second = true;
                        Tok::AndAnd
                    } else {
                        Tok::Amp
                    }
                }
                '|' => {
                    if two(&mut chars, '|') {
                        took_second = true;
                        Tok::OrOr
                    } else {
                        Tok::Pipe
                    }
                }
                _ => return Err(err(format!("unexpected character `{c}`"))),
            };
            if took_second {
                col += 1;
            }
            tok
        };

        tokens.push(Token { tok, pos });
    }

    Ok(tokens)
}

fn keyword(word: &str) -> Option<Tok> {
    Some(match word {
        "fn" => Tok::Fn,
        "var" => Tok::Var,
        "static" => Tok::Static,
        "const" => Tok::Const,
        "use" => Tok::Use,
        "if" => Tok::If,
        "else" => Tok::Else,
        "while" => Tok::While,
        "loop" => Tok::Loop,
        "break" => Tok::Break,
        "continue" => Tok::Continue,
        "return" => Tok::Return,
        "true" => Tok::True,
        "false" => Tok::False,
        "as" => Tok::As,
        "u16" => Tok::U16,
        "i16" => Tok::I16,
        "bool" => Tok::Bool,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        lex(src).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn words_numbers_operators() {
        assert_eq!(
            toks("var x: u16 = 0x2A + 'A';"),
            vec![
                Tok::Var,
                Tok::Ident("x".into()),
                Tok::Colon,
                Tok::U16,
                Tok::Assign,
                Tok::Int(42),
                Tok::Plus,
                Tok::Int(65),
                Tok::Semi
            ]
        );
        assert_eq!(
            toks("a != b && c << 2 -> loop"),
            vec![
                Tok::Ident("a".into()),
                Tok::Ne,
                Tok::Ident("b".into()),
                Tok::AndAnd,
                Tok::Ident("c".into()),
                Tok::Shl,
                Tok::Int(2),
                Tok::Arrow,
                Tok::Loop
            ]
        );
    }

    #[test]
    fn comments_and_positions() {
        let tokens = lex("x // gone\n  y").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].pos, Pos { line: 1, col: 1 });
        assert_eq!(tokens[1].pos, Pos { line: 2, col: 3 });
    }

    #[test]
    fn escapes_in_char_literals() {
        assert_eq!(toks("'\\n'"), vec![Tok::Int(10)]);
        assert_eq!(toks("'\\''"), vec![Tok::Int(39)]);
    }

    #[test]
    fn bad_input_reports_position() {
        let e = lex("var @").unwrap_err();
        assert_eq!((e.line, e.col), (1, 5));
    }
}
