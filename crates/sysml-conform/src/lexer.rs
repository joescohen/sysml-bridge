//! Lexer for KerML clause 8.2.2 (lexical structure), which SysML uses unchanged.
//!
//! The grammar file gives the reserved words and the symbols as lists; the lexer
//! takes both from the file. The other lexical rules are partly prose in the file
//! ("any printable character other than ..."), so they are written by hand here:
//! white space, notes, NAME, DECIMAL_VALUE, EXPONENTIAL_VALUE, STRING_VALUE and
//! REGULAR_COMMENT.

use crate::grammar::Grammar;

/// Kind of a token the grammar has no terminal for (a lexical fault).
pub const BAD: u16 = u16::MAX;

const NAME: usize = 0;
const STRING_VALUE: usize = 1;
const REGULAR_COMMENT: usize = 2;
const DECIMAL_VALUE: usize = 3;
const EXPONENTIAL_VALUE: usize = 4;

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: u16,
    pub line: u32,
    pub column: u32,
    /// Character range in the text.
    pub start: usize,
    pub end: usize,
    /// For a `BAD` token: what is wrong with it.
    pub fault: Option<&'static str>,
}

pub enum LexStop {
    TooManyTokens,
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Escape letters after a backslash. The grammar file lists `\f \n \t \r \v`; the
/// specification's escape table also has `\' \" \b \\`. Both sets are accepted.
fn is_escape(c: char) -> bool {
    matches!(c, 'b' | 't' | 'n' | 'f' | 'r' | 'v' | '\'' | '"' | '\\')
}

struct Cursor<'a> {
    chars: &'a [char],
    at: usize,
    line: u32,
    column: u32,
}

impl Cursor<'_> {
    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    fn bump(&mut self) {
        let c = self.chars[self.at];
        self.at += 1;
        if c == '\n' || (c == '\r' && self.peek(0) != Some('\n')) {
            self.line += 1;
            self.column = 1;
        } else if c != '\r' {
            self.column += 1;
        }
    }

    fn bump_n(&mut self, n: usize) {
        for _ in 0..n {
            self.bump();
        }
    }

    /// Consumes up to and including `*/`. Returns false at end of text.
    fn skip_to_comment_end(&mut self) -> bool {
        while self.at < self.chars.len() {
            if self.peek(0) == Some('*') && self.peek(1) == Some('/') {
                self.bump_n(2);
                return true;
            }
            self.bump();
        }
        false
    }

    /// Consumes a quoted NAME or STRING_VALUE body. The opening quote is consumed.
    fn quoted(&mut self, quote: char) -> Result<(), &'static str> {
        loop {
            match self.peek(0) {
                None => return Err("quoted text is not closed"),
                Some(c) if c == quote => {
                    self.bump();
                    return Ok(());
                }
                Some('\\') => match self.peek(1) {
                    Some(c) if is_escape(c) => self.bump_n(2),
                    _ => {
                        self.bump();
                        return Err("unknown escape sequence in quoted text");
                    }
                },
                Some('\n') | Some('\r') => return Err("quoted text is not closed on its line"),
                Some(c) if c.is_control() => return Err("control character in quoted text"),
                Some(_) => self.bump(),
            }
        }
    }
}

pub fn lex(grammar: &Grammar, chars: &[char], max_tokens: usize) -> Result<Vec<Token>, LexStop> {
    let mut cur = Cursor {
        chars,
        at: 0,
        line: 1,
        column: 1,
    };
    // A byte-order mark is an encoding artefact, not text.
    if cur.peek(0) == Some('\u{feff}') {
        cur.at = 1;
    }
    let mut tokens = Vec::new();
    while let Some(c) = cur.peek(0) {
        if matches!(c, ' ' | '\t' | '\u{c}' | '\r' | '\n') {
            cur.bump();
            continue;
        }
        let (start, line, column) = (cur.at, cur.line, cur.column);
        let mut fault = None;
        let kind = if c == '/' && cur.peek(1) == Some('/') {
            if cur.peek(2) == Some('*') {
                // MULTILINE_NOTE: `//* ... */`
                cur.bump_n(3);
                if cur.skip_to_comment_end() {
                    continue;
                }
                fault = Some("note `//*` is not closed");
                BAD
            } else {
                while !matches!(cur.peek(0), None | Some('\n') | Some('\r')) {
                    cur.bump();
                }
                continue;
            }
        } else if c == '/' && cur.peek(1) == Some('*') {
            cur.bump_n(2);
            if cur.skip_to_comment_end() {
                grammar.class_ids[REGULAR_COMMENT]
            } else {
                fault = Some("comment `/*` is not closed");
                BAD
            }
        } else if is_name_start(c) {
            while cur.peek(0).is_some_and(is_name_char) {
                cur.bump();
            }
            let word: String = chars[start..cur.at].iter().collect();
            grammar
                .keywords
                .get(&word)
                .copied()
                .unwrap_or(grammar.class_ids[NAME])
        } else if c == '\'' || c == '"' {
            cur.bump();
            match cur.quoted(c) {
                Ok(()) if c == '\'' => grammar.class_ids[NAME],
                Ok(()) => grammar.class_ids[STRING_VALUE],
                Err(message) => {
                    fault = Some(message);
                    BAD
                }
            }
        } else if c.is_ascii_digit() {
            while cur.peek(0).is_some_and(|d| d.is_ascii_digit()) {
                cur.bump();
            }
            let mut ahead = 0;
            let mut exponent = false;
            if matches!(cur.peek(0), Some('e') | Some('E')) {
                ahead = 1;
                if matches!(cur.peek(1), Some('+') | Some('-')) {
                    ahead = 2;
                }
                exponent = cur.peek(ahead).is_some_and(|d| d.is_ascii_digit());
            }
            if exponent {
                cur.bump_n(ahead);
                while cur.peek(0).is_some_and(|d| d.is_ascii_digit()) {
                    cur.bump();
                }
                grammar.class_ids[EXPONENTIAL_VALUE]
            } else {
                grammar.class_ids[DECIMAL_VALUE]
            }
        } else {
            let found = grammar
                .symbols
                .iter()
                .find(|(text, _)| chars[start..].starts_with(text))
                .map(|(text, id)| (text.len(), *id));
            match found {
                Some((len, id)) => {
                    cur.bump_n(len);
                    id
                }
                None => {
                    cur.bump();
                    fault = Some("character is not part of any token");
                    BAD
                }
            }
        };
        if tokens.len() == max_tokens {
            return Err(LexStop::TooManyTokens);
        }
        tokens.push(Token {
            kind,
            line,
            column,
            start,
            end: cur.at,
            fault,
        });
        if kind == BAD && fault != Some("character is not part of any token") {
            // After an unclosed quote or comment the rest of the text has no
            // reliable token boundaries.
            break;
        }
    }
    Ok(tokens)
}
