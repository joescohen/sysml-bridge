use crate::model::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokKind {
    Ident,
    /// Unrestricted name written in single quotes.
    Name,
    Number,
    Str,
    /// Block comment `/* ... */`, which is a model element in SysML v2.
    Comment,
    Punct,
    Eof,
}

#[derive(Debug, Clone, Copy)]
pub struct Tok {
    pub kind: TokKind,
    pub span: Span,
}

const PUNCTS: &[&str] = &[
    "===", "!==", ":>>", "::>", "**", "::", ":>", ":=", "..", "->", "=>", "==", "!=", "<=", ">=", "??",
];

/// The character a reader puts in place of bytes that are not UTF-8.
pub const NOT_TEXT: char = '\u{FFFD}';
pub const NOT_TEXT_MESSAGE: &str = "this file is not UTF-8 text: it holds bytes that cannot be read (shown as U+FFFD), so what the tools read is not what is on the disk. Save it as UTF-8";

pub struct LexError {
    pub span: Span,
    pub message: String,
}

/// End of the string or quoted name that starts at `b[start]` (a `"` or a `'`), and whether its closing quote was found.
/// A backslash escapes the next byte, a line break that no backslash precedes ends the token without a closing quote.
/// The lexer and every guard that reads caller text for quotes use this one definition, so they cannot disagree.
pub fn quoted_end(b: &[u8], start: usize) -> (usize, bool) {
    let q = b[start];
    let mut i = start + 1;
    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }
        if b[i] == q {
            return (i + 1, true);
        }
        if b[i] == b'\n' {
            return (i, false);
        }
        i += 1;
    }
    (i.min(b.len()), false)
}

/// End of the identifier that starts at `b[start]` (a letter or `_`).
pub fn ident_end(b: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
        i += 1;
    }
    i
}

/// End of the number that starts at `b[start]` (a digit): digits, a fraction when a digit follows the dot (so `1..5`
/// stays a range), an exponent when digits follow it. Letters after the number start a new identifier (`5if` is `5`, `if`).
pub fn number_end(b: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i + 1 < b.len() && b[i] == b'.' && b[i + 1].is_ascii_digit() {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    i
}

/// `b[start..end]` is a number token of digits only and a letter or `_` follows it at once: the start of a name like
/// `5out`. A number with a fraction or an exponent (`2.5`, `1e5`) is never a name.
fn digit_name_at(b: &[u8], start: usize, end: usize) -> bool {
    end < b.len() && (b[end].is_ascii_alphabetic() || b[end] == b'_') && b[start..end].iter().all(u8::is_ascii_digit)
}

/// The text with every bare name that starts with a digit (`port 5out : 2DataPort;`) written in quotes
/// (`port '5out' : '2DataPort';`), as the official grammar reads it; `None` when there is none. Strings, quoted names
/// and comments are left as they are. A person sees the name without quotes; only the file holds them.
pub fn quote_digit_names(src: &str) -> Option<String> {
    let (toks, _) = lex(src);
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len() + 8);
    let (mut at, mut found) = (0, false);
    for t in toks.iter().filter(|t| t.kind == TokKind::Ident && b.get(t.span.start).is_some_and(u8::is_ascii_digit)) {
        found = true;
        out.push_str(&src[at..t.span.start]);
        out.push('\'');
        out.push_str(&src[t.span.start..t.span.end]);
        out.push('\'');
        at = t.span.end;
    }
    if !found {
        return None;
    }
    out.push_str(&src[at..]);
    Some(out)
}

pub fn lex(src: &str) -> (Vec<Tok>, Vec<LexError>) {
    let b = src.as_bytes();
    let mut toks = Vec::new();
    let mut errs = Vec::new();
    // A file that is not UTF-8 text reaches the lexer with U+FFFD where its bytes could not be read. That is an error
    // wherever it stands (in a comment and in a string too): the text read is not the text on the disk.
    if let Some(at) = src.find(NOT_TEXT) {
        errs.push(LexError { span: Span::new(at, at + NOT_TEXT.len_utf8()), message: NOT_TEXT_MESSAGE.into() });
    }
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            // `//` is a note and is not part of the model. `//* ... */` is a block note.
            if b.get(i + 2) == Some(&b'*') {
                i += 3;
                while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                    i += 1;
                }
                i = (i + 2).min(b.len());
                continue;
            }
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            let mut closed = false;
            while i < b.len() {
                if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    i += 2;
                    closed = true;
                    break;
                }
                i += 1;
            }
            if !closed {
                errs.push(LexError { span: Span::new(start, i), message: "unterminated comment".into() });
            }
            toks.push(Tok { kind: TokKind::Comment, span: Span::new(start, i) });
            continue;
        }
        if c == b'\'' || c == b'"' {
            let (end, closed) = quoted_end(b, i);
            i = end;
            if !closed {
                errs.push(LexError { span: Span::new(start, i), message: "unterminated string or name".into() });
            }
            let kind = if c == b'\'' { TokKind::Name } else { TokKind::Str };
            toks.push(Tok { kind, span: Span::new(start, i) });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            i = ident_end(b, i);
            toks.push(Tok { kind: TokKind::Ident, span: Span::new(start, i) });
            continue;
        }
        if c.is_ascii_digit() {
            i = number_end(b, i);
            // A digit run that runs straight into letters (`5out`, `2DataPort`) is a name that starts with a digit:
            // read as that name. The file form is quoted ('5out'); every write path quotes it (quote_digit_names).
            if digit_name_at(b, start, i) {
                i = ident_end(b, i);
                toks.push(Tok { kind: TokKind::Ident, span: Span::new(start, i) });
                continue;
            }
            toks.push(Tok { kind: TokKind::Number, span: Span::new(start, i) });
            continue;
        }
        if c >= 0x80 {
            // Non-ASCII outside a string or name: skip the whole character.
            let ch = src[i..].chars().next().unwrap();
            i += ch.len_utf8();
            errs.push(LexError { span: Span::new(start, i), message: format!("unexpected character '{ch}'") });
            continue;
        }
        let rest = &src[i..];
        let len = PUNCTS.iter().find(|p| rest.starts_with(**p)).map(|p| p.len()).unwrap_or(1);
        i += len;
        toks.push(Tok { kind: TokKind::Punct, span: Span::new(start, i) });
    }
    toks.push(Tok { kind: TokKind::Eof, span: Span::new(b.len(), b.len()) });
    (toks, errs)
}

/// Remove the quotes and escapes from a quoted name or string token. The closing quote is optional:
/// a token that the file cuts off keeps all its characters (the last one may be several bytes long).
pub fn unquote(raw: &str) -> String {
    let mut chars = raw.chars();
    let open = match chars.next() {
        Some(c @ ('\'' | '"')) if raw.len() >= 2 => c,
        _ => return raw.to_string(),
    };
    let mut out = String::with_capacity(raw.len());
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(o) => out.push(o),
                None => {}
            },
            c if c == open => break,
            c => out.push(c),
        }
    }
    out
}

/// Turn the raw text of a block comment into its body.
pub fn clean_comment(raw: &str) -> String {
    let inner = raw.strip_prefix("/*").unwrap_or(raw);
    let inner = inner.strip_suffix("*/").unwrap_or(inner);
    let lines: Vec<String> = inner
        .lines()
        .map(|l| {
            let t = l.trim();
            let t = t.strip_prefix('*').map(|r| r.strip_prefix(' ').unwrap_or(r)).unwrap_or(t);
            t.trim_end().to_string()
        })
        .collect();
    lines.join("\n").trim().to_string()
}
