//! Reader for the `.kebnf` notation the OMG release uses for its textual grammars.
//!
//! The reader keeps only what decides whether a text is in the language:
//! terminals, rule references, grouping, alternation and repetition. It drops the
//! abstract-syntax annotations, which say what to build and not what to accept:
//!   - the `: Type` after a rule name,
//!   - `property =`, `property +=` and `property ?=` in front of a value,
//!   - `[Rule]` and `~[Rule]` cross-references, which become plain `Rule`,
//!   - `{ ... }` actions, which match the empty string.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A quoted terminal, without its quotes.
    Term(String),
    /// A reference to another rule or to a lexical token class.
    Ref(String),
    Seq(Vec<Expr>),
    Alt(Vec<Expr>),
    Opt(Box<Expr>),
    Star(Box<Expr>),
    Plus(Box<Expr>),
}

#[derive(Debug, Clone)]
pub struct RuleDef {
    pub name: String,
    pub body: Expr,
    /// 1-based line of the rule header in its file.
    pub line: usize,
    /// The `//` comment lines directly above the rule header.
    pub comment: String,
}

#[derive(Debug)]
pub struct KebnfError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for KebnfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "grammar line {}: {}", self.line, self.message)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Quoted(String),
    Ident(String),
    Action,
    Assign,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Bar,
    Question,
    Star,
    Plus,
    Tilde,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A rule starts at column 0 with `Name =` or `Name : Type =`. Both release files
/// keep every body line indented, so the column is a safe marker.
fn header(line: &str) -> Option<(String, usize)> {
    let mut chars = line.char_indices().peekable();
    let (_, first) = *chars.peek()?;
    if !is_ident_start(first) {
        return None;
    }
    let mut end = 0;
    while let Some(&(i, c)) = chars.peek() {
        if is_ident_char(c) {
            end = i + c.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    let name = line[..end].to_string();
    let rest = line[end..].trim_start();
    let rest = if let Some(after) = rest.strip_prefix(':') {
        let after = after.trim_start();
        let type_len = after.chars().take_while(|c| is_ident_char(*c)).count();
        if type_len == 0 {
            return None;
        }
        after[type_len..].trim_start()
    } else {
        rest
    };
    let body = rest.strip_prefix('=')?;
    Some((name, line.len() - body.len()))
}

fn tokenize(text: &str, first_line: usize) -> Result<Vec<(Tok, usize)>, KebnfError> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut line = first_line;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            c if c.is_whitespace() => i += 1,
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\'' => {
                let start = i + 1;
                let mut j = start;
                while j < chars.len() && chars[j] != '\'' && chars[j] != '\n' {
                    j += 1;
                }
                if chars.get(j) != Some(&'\'') {
                    return Err(KebnfError {
                        line,
                        message: "quoted terminal is not closed".into(),
                    });
                }
                out.push((Tok::Quoted(chars[start..j].iter().collect()), line));
                i = j + 1;
            }
            '{' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '}' {
                    if chars[j] == '\n' {
                        line += 1;
                    }
                    j += 1;
                }
                if j == chars.len() {
                    return Err(KebnfError {
                        line,
                        message: "action `{` is not closed".into(),
                    });
                }
                out.push((Tok::Action, line));
                i = j + 1;
            }
            c if is_ident_start(c) => {
                let start = i;
                while i < chars.len() && is_ident_char(chars[i]) {
                    i += 1;
                }
                out.push((Tok::Ident(chars[start..i].iter().collect()), line));
            }
            '+' if chars.get(i + 1) == Some(&'=') => {
                out.push((Tok::Assign, line));
                i += 2;
            }
            '?' if chars.get(i + 1) == Some(&'=') => {
                out.push((Tok::Assign, line));
                i += 2;
            }
            '=' => {
                out.push((Tok::Assign, line));
                i += 1;
            }
            '(' | ')' | '[' | ']' | '|' | '?' | '*' | '+' | '~' => {
                out.push((
                    match c {
                        '(' => Tok::LParen,
                        ')' => Tok::RParen,
                        '[' => Tok::LBracket,
                        ']' => Tok::RBracket,
                        '|' => Tok::Bar,
                        '?' => Tok::Question,
                        '*' => Tok::Star,
                        '+' => Tok::Plus,
                        _ => Tok::Tilde,
                    },
                    line,
                ));
                i += 1;
            }
            other => {
                return Err(KebnfError {
                    line,
                    message: format!("unexpected character `{other}`"),
                });
            }
        }
    }
    Ok(out)
}

struct BodyParser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    last_line: usize,
}

impl BodyParser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.0)
    }

    fn line(&self) -> usize {
        self.toks
            .get(self.pos)
            .map(|t| t.1)
            .unwrap_or(self.last_line)
    }

    fn err<T>(&self, message: &str) -> Result<T, KebnfError> {
        Err(KebnfError {
            line: self.line(),
            message: message.into(),
        })
    }

    fn alt(&mut self) -> Result<Expr, KebnfError> {
        let mut alts = vec![self.seq()?];
        while self.peek() == Some(&Tok::Bar) {
            self.pos += 1;
            alts.push(self.seq()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap()
        } else {
            Expr::Alt(alts)
        })
    }

    fn seq(&mut self) -> Result<Expr, KebnfError> {
        let mut items = Vec::new();
        while !matches!(self.peek(), None | Some(Tok::Bar) | Some(Tok::RParen)) {
            items.push(self.item()?);
        }
        Ok(if items.len() == 1 {
            items.pop().unwrap()
        } else {
            Expr::Seq(items)
        })
    }

    fn item(&mut self) -> Result<Expr, KebnfError> {
        let mut expr = self.primary()?;
        loop {
            expr = match self.peek() {
                Some(Tok::Question) => Expr::Opt(Box::new(expr)),
                Some(Tok::Star) => Expr::Star(Box::new(expr)),
                Some(Tok::Plus) => Expr::Plus(Box::new(expr)),
                _ => return Ok(expr),
            };
            self.pos += 1;
        }
    }

    fn primary(&mut self) -> Result<Expr, KebnfError> {
        let Some(tok) = self.peek().cloned() else {
            return self.err("rule body ends where a value is needed");
        };
        self.pos += 1;
        match tok {
            Tok::Quoted(text) => Ok(Expr::Term(text)),
            Tok::Action => Ok(Expr::Seq(Vec::new())),
            Tok::LParen => {
                let inner = self.alt()?;
                if self.peek() != Some(&Tok::RParen) {
                    return self.err("`(` is not closed");
                }
                self.pos += 1;
                Ok(inner)
            }
            Tok::Tilde | Tok::LBracket => {
                if tok == Tok::Tilde {
                    if self.peek() != Some(&Tok::LBracket) {
                        return self.err("`~` must be followed by `[Rule]`");
                    }
                    self.pos += 1;
                }
                let Some(Tok::Ident(name)) = self.peek().cloned() else {
                    return self.err("`[` must hold a rule name");
                };
                self.pos += 1;
                if self.peek() != Some(&Tok::RBracket) {
                    return self.err("`[` is not closed");
                }
                self.pos += 1;
                Ok(Expr::Ref(name))
            }
            Tok::Ident(name) => {
                if self.peek() == Some(&Tok::Assign) {
                    // `property = value`: the property is abstract syntax, the value is text.
                    self.pos += 1;
                    self.primary()
                } else {
                    Ok(Expr::Ref(name))
                }
            }
            _ => {
                self.pos -= 1;
                self.err("unexpected symbol in rule body")
            }
        }
    }
}

/// Reads every rule of one `.kebnf` file, in file order.
pub fn parse(source: &str) -> Result<Vec<RuleDef>, KebnfError> {
    struct Pending {
        name: String,
        line: usize,
        comment: String,
        body: String,
    }
    let mut pending: Vec<Pending> = Vec::new();
    let mut comment = String::new();
    for (index, line) in source.lines().enumerate() {
        if let Some((name, body_at)) = header(line) {
            pending.push(Pending {
                name,
                line: index + 1,
                comment: std::mem::take(&mut comment),
                body: line[body_at..].to_string(),
            });
            continue;
        }
        if let Some(text) = line.strip_prefix("//") {
            if !comment.is_empty() {
                comment.push('\n');
            }
            comment.push_str(text.trim());
        } else {
            comment.clear();
        }
        if let Some(rule) = pending.last_mut() {
            rule.body.push('\n');
            rule.body.push_str(line);
        }
    }
    let mut rules = Vec::with_capacity(pending.len());
    for rule in pending {
        let toks = tokenize(&rule.body, rule.line)?;
        let last_line = toks.last().map(|t| t.1).unwrap_or(rule.line);
        let mut parser = BodyParser {
            toks,
            pos: 0,
            last_line,
        };
        let body = parser.alt()?;
        if parser.pos != parser.toks.len() {
            return parser.err("unexpected `)` in rule body");
        }
        rules.push(RuleDef {
            name: rule.name,
            body,
            line: rule.line,
            comment: rule.comment,
        });
    }
    Ok(rules)
}

/// The quoted terminals of a rule whose body is a plain list of alternatives,
/// such as `RESERVED_KEYWORD`.
pub fn terminal_list(rule: &RuleDef) -> Vec<String> {
    fn walk(expr: &Expr, out: &mut Vec<String>) {
        match expr {
            Expr::Term(text) => out.push(text.clone()),
            Expr::Seq(items) | Expr::Alt(items) => items.iter().for_each(|e| walk(e, out)),
            Expr::Opt(inner) | Expr::Star(inner) | Expr::Plus(inner) => walk(inner, out),
            Expr::Ref(_) => {}
        }
    }
    let mut out = Vec::new();
    walk(&rule.body, &mut out);
    out
}
