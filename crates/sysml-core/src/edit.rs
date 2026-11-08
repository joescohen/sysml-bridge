//! Model edits applied as text patches, so text stays the source of truth.

use crate::model::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    /// Insert SysML text into the body of `owner`, or at the end of `file`.
    AddElement { owner: Option<String>, file: Option<String>, text: String },
    /// Rename a declaration and every reference to it.
    Rename { target: String, new_name: String },
    Delete { target: String },
    /// Replace the whole statement of `target` with new text.
    Replace { target: String, text: String },
    SetDoc { target: String, doc: String },
    AddSpecialization { target: String, general: String },
    /// Set the type of a usage (`: T`), replacing the current one.
    SetType { target: String, type_ref: String },
    /// Set the multiplicity of a usage; an empty string removes it.
    SetMultiplicity { target: String, multiplicity: String },
    /// Set the value (`= v`) of a usage; an empty string removes it.
    SetValue { target: String, value: String },
    /// Set the short name (`<'R-1'>`); an empty string removes it.
    SetShortName { target: String, short: String },
    /// Remove the `index`-th specialization (`:> T`).
    RemoveSpecialization { target: String, index: usize },
    /// Replace all redefinitions (`:>> a, b`) with `names`; an empty list removes them.
    SetRedefines { target: String, names: Vec<String> },
    /// Remove the typing (`: T`).
    ClearType { target: String },
    /// Replace all reference subsettings (`::> a, b`) with `names`; an empty list removes them.
    SetReferences { target: String, names: Vec<String> },
    /// Replace the leading metadata prefixes (`#Pkg::Tag`) with `tags`; an empty list removes them.
    SetMetadata { target: String, tags: Vec<String> },
    /// Replace the modifier words and the direction before the keyword (`private ref in part x`), and the words
    /// after the name (`ordered`, `nonunique`, `parallel`). `words: None` keeps the words, `direction: Some("")`
    /// removes the direction; `#tags` stay where they are.
    SetPrefixes { target: String, direction: Option<String>, words: Option<Vec<String>> },
}

/// Visibility words. At most one may stand before a keyword; it is written first.
pub const VISIBILITY: &[&str] = &["public", "private", "protected"];

/// Modifier words that stand before the keyword, in the order they are written (after visibility, the `#tags` and the direction).
pub const LEADING_MODIFIERS: &[&str] = &[
    "member", "variant", "return", "individual", "snapshot", "timeslice", "portion", "derived", "abstract", "variation", "constant", "var",
    "composite", "ref", "end", "event", "then", "library",
];

/// Modifier words written after the name and its typing: `ordered` and `nonunique` follow the multiplicity, `parallel` ends the heading.
pub const TAIL_MODIFIERS: &[&str] = &["ordered", "nonunique", "parallel"];

/// Where a modifier word is written: visibility first, then the other leading words, then the tail words.
pub fn modifier_rank(w: &str) -> usize {
    VISIBILITY
        .iter()
        .chain(LEADING_MODIFIERS)
        .chain(TAIL_MODIFIERS)
        .position(|x| *x == w)
        .unwrap_or(usize::MAX)
}

fn ident_at(s: &str, at: usize) -> &str {
    let rest = &s[at..];
    let n = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
    &rest[..n]
}

/// The text before the keyword: visibility, the `#tags` (already with a space after each), the direction, the other words.
fn head_text(words: &[String], metas: &str, dir: Option<&str>) -> String {
    let mut words = words.to_vec();
    words.sort_by_key(|w| modifier_rank(w));
    let mut head = String::new();
    for w in words.iter().filter(|w| VISIBILITY.contains(&w.as_str())) {
        head.push_str(&format!("{w} "));
    }
    head.push_str(metas);
    if let Some(d) = dir {
        head.push_str(&format!("{d} "));
    }
    for w in words.iter().filter(|w| !VISIBILITY.contains(&w.as_str())) {
        head.push_str(&format!("{w} "));
    }
    head
}

/// Leading words of a statement: `(end of the last one and the white space after it, #tags, direction, words)`.
fn scan_prefixes(stmt: &str) -> (usize, String, Option<String>, Vec<String>) {
    let (mut at, mut metas, mut dir, mut words) = (0usize, String::new(), None, Vec::new());
    loop {
        let j = at + (stmt.len() - at - stmt[at..].trim_start().len());
        if stmt[j..].starts_with('#') {
            let n = leading_metadata_len(&stmt[j..]);
            if n == 0 {
                break;
            }
            metas.push_str(stmt[j..j + n].trim_end());
            metas.push(' ');
            at = j + n;
            continue;
        }
        let w = ident_at(stmt, j);
        if matches!(w, "in" | "out" | "inout") {
            dir = Some(w.to_string());
        } else if VISIBILITY.contains(&w) || LEADING_MODIFIERS.contains(&w) || w == "standard" {
            words.push(w.to_string());
        } else {
            return (j, metas, dir, words);
        }
        at = j + w.len();
    }
    (at, metas, dir, words)
}

/// Spans of the tail words (`ordered`, `nonunique`, `parallel`) in the heading of `e`, each with the space before it,
/// and the end of the heading's declaration part (before `=`, `{` or `;`).
fn tail_words(text: &str, e: &Element) -> Result<(Vec<(Span, String)>, usize), String> {
    let from = e.name_span.filter(|_| !e.implied_name).ok_or("the element has no declared name, so words after the name cannot be changed")?.end;
    let b = text.as_bytes();
    let (mut i, mut depth, mut found) = (from, 0i32, Vec::new());
    let limit = e.span.end.min(text.len());
    while i < limit {
        match b[i] {
            b'\'' => {
                i += 1;
                while i < limit && b[i] != b'\'' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth -= 1,
            b'=' | b';' | b'{' if depth == 0 => break,
            c if depth == 0 && (c.is_ascii_alphabetic() || c == b'_') => {
                let before = text[..i].chars().next_back();
                let w = ident_at(text, i);
                let bounded = !before.map(|c| c.is_alphanumeric() || c == '_' || c == ':' || c == '.' || c == '~').unwrap_or(false);
                if bounded && TAIL_MODIFIERS.contains(&w) {
                    let start = if text[..i].ends_with(' ') { i - 1 } else { i };
                    found.push((Span::new(start, i + w.len()), w.to_string()));
                }
                i += w.len();
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    let end = text[from..i.min(limit)].trim_end().len() + from;
    Ok((found, end))
}

/// Length of the `#Pkg::Tag ` prefixes at the start of `text`, with the spaces after each.
pub fn leading_metadata_len(text: &str) -> usize {
    let b = text.as_bytes();
    let mut i = 0;
    loop {
        if b.get(i) != Some(&b'#') {
            return i;
        }
        let mut j = i + 1;
        loop {
            if b.get(j) == Some(&b'\'') {
                j += 1;
                while j < b.len() && b[j] != b'\'' {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                j += 1;
            } else {
                let from = j;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
                if j == from {
                    return i;
                }
            }
            if text.get(j..j + 2) == Some("::") {
                j += 2;
            } else {
                break;
            }
        }
        while b.get(j).map(|c| c.is_ascii_whitespace()).unwrap_or(false) {
            j += 1;
        }
        i = j;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    pub path: String,
    pub text: String,
}

struct Patch {
    file: usize,
    span: Span,
    text: String,
}

/// Bytes of new text one edit may write. A rename writes its name once per use, so this bounds the file it makes.
pub const MAX_ADDED_BYTES: usize = 8 << 20;
/// Longest name an edit may write.
pub const MAX_NAME_BYTES: usize = 2000;

const STALE: &str = "the model and the file text disagree: a span lies outside the text or inside a character. Read the model again and retry";

/// `text[a..b]`, or an error when the range is outside the text or cuts a character in two.
fn sub(text: &str, a: usize, b: usize) -> Result<&str, String> {
    text.get(a..b).ok_or_else(|| STALE.to_string())
}

/// `text[..at]`.
fn upto(text: &str, at: usize) -> Result<&str, String> {
    sub(text, 0, at)
}

/// The first 80 characters of `s`, escaped, for a message. Text from a caller can be as long as a request.
pub fn clip(s: &str) -> String {
    let head: String = s.chars().take(80).collect();
    let more = if s.chars().nth(80).is_some() { "..." } else { "" };
    format!("{}{more}", head.escape_debug())
}

fn file_text(model: &Model, file: usize) -> Result<&str, String> {
    model.files.get(file).map(|f| f.text.as_str()).ok_or_else(|| STALE.to_string())
}

fn line_indent(text: &str, offset: usize) -> Result<String, String> {
    let start = upto(text, offset)?.rfind('\n').map(|i| i + 1).unwrap_or(0);
    Ok(sub(text, start, text.len())?.chars().take_while(|c| *c == ' ' || *c == '\t').collect())
}

/// Characters that change how text reads or where a line ends without showing, by Unicode category: format characters
/// (Cf: bidi controls, zero-width characters, the byte order mark, tags), line and paragraph separators (Zl, Zp), private
/// use (Co), code points that are not assigned (Cn, which holds the noncharacters) and every default-ignorable code point
/// (soft hyphen, variation selectors, Hangul fillers). The table is `hidden_chars::HIDDEN`.
pub fn is_invisible(c: char) -> bool {
    let cp = c as u32;
    if cp < 0xAD {
        return false;
    }
    let t = crate::hidden_chars::HIDDEN;
    let i = t.partition_point(|(_, hi)| *hi < cp);
    t.get(i).map(|(lo, _)| *lo <= cp).unwrap_or(false)
}

/// The one test for a character a tool is about to write. `Some(what it is)` for a character that must not go into a
/// name or a file path (`name` set) or into a value or documentation (`name` clear); `None` for one that may.
/// Both refuse control characters and the characters of `is_invisible`. A name and a path are one line and their only
/// space is U+0020: a no-break space or an ideographic space in a name looks like the plain one and is another name.
/// Text keeps the line break, the tab and the carriage return (the caller checks that a line feed follows it).
pub fn char_fault(c: char, name: bool) -> Option<&'static str> {
    if c.is_control() {
        return if !name && matches!(c, '\n' | '\t' | '\r') { None } else { Some("a control character") };
    }
    if is_invisible(c) {
        return Some("a format, separator, private-use, unassigned or default-ignorable character, which does not show");
    }
    if name && c != ' ' && c.is_whitespace() {
        return Some("a space other than the plain space U+0020");
    }
    None
}

/// The first character of `t` that a name or a file path must not hold.
pub fn name_fault(t: &str) -> Option<(char, &'static str)> {
    t.chars().find_map(|c| char_fault(c, true).map(|w| (c, w)))
}

/// The characters of `t` a value or document must not gain: those `char_fault` refuses in text, and a carriage return that
/// is not followed by a line feed.
fn bad_chars(t: &str) -> impl Iterator<Item = char> + '_ {
    let b = t.as_bytes();
    t.char_indices().filter(move |(i, c)| char_fault(*c, false).is_some() || (*c == '\r' && b.get(i + 1) != Some(&b'\n'))).map(|(_, c)| c)
}

/// Number of such characters in `t`.
pub fn count_bad_chars(t: &str) -> usize {
    bad_chars(t).count()
}

/// The first such character, for a message.
pub fn first_bad_char(t: &str) -> Option<char> {
    bad_chars(t).next()
}

/// The first such character that `new` holds more often than `old`. Each character is counted by itself: a text that
/// drops one hidden character and gains another has gained one, although the number of them is the same.
pub fn gained_bad_char(old: &str, new: &str) -> Option<char> {
    let mut have: std::collections::HashMap<char, i64> = std::collections::HashMap::new();
    for c in bad_chars(old) {
        *have.entry(c).or_default() += 1;
    }
    bad_chars(new).find(|c| {
        let n = have.entry(*c).or_default();
        *n -= 1;
        *n < 0
    })
}

/// A qualified name as the one place that writes caller text as a type, general type, item or import target.
/// It accepts segments that are plain names or quoted names joined by `::`, an optional leading `~`, and, when
/// `wild` is set, a final `::*` or `::**`; it returns the canonical spelling. Anything else is refused, so caller text
/// never reaches the file unchecked.
pub fn qualified_text(what: &str, given: &str, wild: bool) -> Result<String, String> {
    let g = given.trim();
    if g.len() > MAX_NAME_BYTES {
        return Err(format!("{what} is {} bytes, which is too long; at most {MAX_NAME_BYTES}", g.len()));
    }
    if let Some((c, why)) = name_fault(g) {
        return Err(format!("{what} holds the character U+{:04X}, {why}. Fix: pass a qualified name such as Pkg::Name.", c as u32));
    }
    let bad = || format!("{what} {} is not a qualified name: use segments that are plain names or quoted names joined by '::', for example Pkg::'My Part'. Fix: pass one name, with no punctuation, braces, or statements.", crate::agent::text::quote_clip(g));
    let (conj, rest) = match g.strip_prefix('~') {
        Some(r) => ("~", r.trim_start()),
        None => ("", g),
    };
    let (rest, suffix) = match (wild, rest.strip_suffix("::**"), rest.strip_suffix("::*")) {
        (true, Some(r), _) => (r, "::**"),
        (true, None, Some(r)) => (r, "::*"),
        _ => (rest, ""),
    };
    let mut segs: Vec<String> = Vec::new();
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    loop {
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        let mut name = String::new();
        if i < chars.len() && chars[i] == '\'' {
            i += 1;
            loop {
                match chars.get(i) {
                    None => return Err(bad()),
                    Some('\\') => match chars.get(i + 1) {
                        Some(c @ ('\'' | '\\')) => {
                            name.push(*c);
                            i += 2;
                        }
                        _ => return Err(bad()),
                    },
                    Some('\'') => {
                        i += 1;
                        break;
                    }
                    Some(c) => {
                        name.push(*c);
                        i += 1;
                    }
                }
            }
        } else {
            let s = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '$') {
                i += 1;
            }
            if i == s {
                return Err(bad());
            }
            name = chars[s..i].iter().collect();
        }
        if name.is_empty() {
            return Err(bad());
        }
        segs.push(name);
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        if i == chars.len() {
            break;
        }
        if chars[i] == ':' && chars.get(i + 1) == Some(&':') {
            i += 2;
        } else {
            return Err(bad());
        }
    }
    let joined = segs.iter().map(|s| if s.starts_with('$') && s.len() > 1 && s[1..].chars().all(|c| c.is_ascii_digit()) { s.clone() } else { quote_name(s) }).collect::<Vec<_>>().join("::");
    Ok(format!("{conj}{joined}{suffix}"))
}

/// A name that is safe to write inside quotes: not empty, not long, one line.
pub fn checked_name<'a>(what: &str, name: &'a str) -> Result<&'a str, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err(format!("{what} is empty"));
    }
    if n.len() > MAX_NAME_BYTES {
        return Err(format!("{what} is {} bytes, which is too long; at most {MAX_NAME_BYTES}", n.len()));
    }
    if let Some((c, why)) = name_fault(n) {
        return Err(format!("{what} has a control character, an invisible character or a line break (U+{:04X}, {why})", c as u32));
    }
    Ok(n)
}

/// Documentation may hold line breaks, carriage returns and tabs; any other control character is refused.
pub fn check_doc_text(what: &str, text: &str) -> Result<(), String> {
    match text.chars().find(|c| char_fault(*c, false).is_some()) {
        Some(c) => Err(format!("{what} holds the control character U+{:04X}. Fix: remove it; only line breaks and tabs are kept.", c as u32)),
        None => Ok(()),
    }
}

fn indent_block(snippet: &str, indent: &str) -> String {
    snippet
        .trim()
        .lines()
        .map(|l| if l.trim().is_empty() { String::new() } else { format!("{indent}{l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

fn find<'a>(model: &'a Model, qn: &str) -> Result<&'a Element, String> {
    model.find(qn).ok_or_else(|| format!("no element named {}", crate::agent::text::quote_clip(qn)))
}

fn add_into(model: &Model, owner: &Element, snippet: &str) -> Result<Patch, String> {
    let text = file_text(model, owner.file)?;
    let indent = line_indent(text, owner.span.start)?;
    let child_indent = format!("{indent}    ");
    let block = indent_block(snippet, &child_indent);
    if let Some(body) = owner.body_span {
        // A body ends with its `}` unless the file stops first; then there is no place to put a member.
        let braces = body.end >= body.start + 2 && text.get(body.start..).is_some_and(|t| t.starts_with('{')) && upto(text, body.end)?.ends_with('}');
        if !braces {
            return Err(format!("'{}' has a '{{' with no closing '}}', so there is nowhere to add to it. Fix: close the body first", owner.qualified_name));
        }
        let inner = sub(text, body.start + 1, body.end - 1)?;
        if inner.trim().is_empty() {
            return Ok(Patch {
                file: owner.file,
                span: Span::new(body.start + 1, body.end - 1),
                text: format!("\n{block}\n{indent}"),
            });
        }
        let at = body.start + 1 + inner.trim_end().len();
        return Ok(Patch { file: owner.file, span: Span::new(at, at), text: format!("\n{block}") });
    }
    let stmt = sub(text, owner.span.start, owner.span.end)?;
    if !stmt.ends_with(';') {
        return Err(format!("'{}' has no body to add to", owner.qualified_name));
    }
    let at = owner.span.end - 1;
    let lead = if upto(text, at)?.ends_with(' ') { "" } else { " " };
    Ok(Patch { file: owner.file, span: Span::new(at, at + 1), text: format!("{lead}{{\n{block}\n{indent}}}") })
}

/// Offset just after the name and the typing, specialization and
/// redefinition parts that follow it. New `: T` or `[m]` text goes here.
fn tail_end(e: &Element) -> Result<usize, String> {
    let name = e.name_span.filter(|_| !e.implied_name).ok_or("element has no declared name")?;
    let mut end = name.end;
    for r in &e.refs {
        let tail = matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines | RefRole::References);
        if tail && r.span.start >= name.end && r.span.end > end {
            end = r.span.end;
        }
    }
    Ok(end)
}

/// Span of the first top-level `[...]` after the name, if the element has one.
fn multiplicity_span(text: &str, e: &Element) -> Option<Span> {
    let from = e.name_span?.end;
    let limit = e.body_span.map(|b| b.start).unwrap_or(e.span.end).min(text.len());
    let bytes = text.as_bytes();
    let mut i = from;
    while i < limit {
        match bytes[i] {
            b'\'' => {
                i += 1;
                while i < limit && bytes[i] != b'\'' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'=' | b';' | b'{' => return None,
            b'[' => {
                let mut depth = 0;
                for (j, c) in text.get(i..limit)?.char_indices() {
                    match c {
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                return Some(Span::new(i, i + j + 1));
                            }
                        }
                        _ => {}
                    }
                }
                return None;
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// End of the heading of a statement: before its `{` body or its closing `;`, without trailing space.
fn head_end(text: &str, e: &Element) -> Result<usize, String> {
    let end = match e.body_span {
        Some(b) => b.start,
        None if upto(text, e.span.end)?.ends_with(';') => e.span.end - 1,
        None => e.span.end,
    };
    // `trim_end` drops white space of any width; stepping back one byte at a time would stop inside a character.
    Ok(e.span.start + sub(text, e.span.start, end)?.trim_end().len())
}

/// Start of a keyword or operator that ends just before `at`, with the space before it; `None` when it is not there.
fn operator_before(text: &str, at: usize, ops: &[&str]) -> Option<usize> {
    let before = text.get(..at)?.trim_end();
    let op = ops.iter().find(|o| before.ends_with(**o))?;
    let mut start = before.len() - op.len();
    while text.get(..start)?.ends_with(' ') {
        start -= 1;
    }
    Some(start)
}

/// Span of the `<'R-1'>` before a name inside `region` (the text from the start of the statement to the name).
fn short_name_span(region: &str) -> Option<Span> {
    let b = region.as_bytes();
    // End of a quoted name that starts at `i`.
    let quoted_end = |mut i: usize| {
        i += 1;
        while i < b.len() && b[i] != b'\'' {
            i += if b[i] == b'\\' { 2 } else { 1 };
        }
        i + 1
    };
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\'' => i = quoted_end(i),
            b'<' => {
                let mut j = if b.get(i + 1) == Some(&b'\'') { quoted_end(i + 1) } else { b[i + 1..].iter().position(|c| *c == b'>').map(|p| i + 1 + p).unwrap_or(b.len()) };
                while b.get(j) == Some(&b' ') {
                    j += 1;
                }
                return (b.get(j) == Some(&b'>')).then(|| Span::new(i, j + 1));
            }
            _ => i += 1,
        }
    }
    None
}

pub fn apply(model: &Model, op: &EditOp) -> Result<Vec<FileChange>, String> {
    let mut patches: Vec<Patch> = Vec::new();
    let mut new_file: Option<FileChange> = None;
    match op {
        EditOp::AddElement { owner: Some(owner), text, .. } if !owner.is_empty() => {
            patches.push(add_into(model, find(model, owner)?, text)?);
        }
        EditOp::AddElement { file, text, .. } => {
            let path = file.clone().or_else(|| model.files.first().map(|f| f.path.clone())).unwrap_or("model.sysml".into());
            match model.files.iter().position(|f| f.path == path) {
                Some(i) => {
                    let t = &model.files[i].text;
                    let end = t.trim_end().len();
                    let sep = if end == 0 { "" } else { "\n\n" };
                    patches.push(Patch { file: i, span: Span::new(end, t.len()), text: format!("{sep}{}\n", text.trim()) });
                }
                None => new_file = Some(FileChange { path, text: format!("{}\n", text.trim()) }),
            }
        }
        EditOp::Rename { target, new_name } => {
            let e = find(model, target)?;
            let span = e.name_span.filter(|_| !e.implied_name).ok_or("element has no declared name to rename")?;
            let old_name = e.name.as_deref().ok_or("element has no declared name to rename")?;
            let quoted = quote_name(checked_name("new name", new_name)?);
            patches.push(Patch { file: e.file, span, text: quoted.clone() });
            for other in &model.elements {
                for r in &other.refs {
                    for seg in &r.segments {
                        if seg.resolved == Some(e.id) && seg.name == old_name {
                            patches.push(Patch { file: other.file, span: seg.span, text: quoted.clone() });
                        }
                    }
                }
            }
        }
        EditOp::Delete { target } => patches.push(delete_patch(model, find(model, target)?)?),
        EditOp::Replace { target, text } => {
            let e = find(model, target)?;
            let indent = line_indent(file_text(model, e.file)?, e.span.start)?;
            let block = indent_block(text, &indent);
            patches.push(Patch { file: e.file, span: e.span, text: block.trim_start().to_string() });
        }
        EditOp::SetDoc { target, doc } => {
            let e = find(model, target)?;
            check_doc_text("documentation", doc)?;
            let snippet = format!("doc /* {} */", doc.trim().replace("*/", "* /"));
            match model.children(e.id).find(|c| c.kind == "doc") {
                Some(d) => patches.push(Patch { file: d.file, span: d.span, text: snippet }),
                None => patches.push(add_into(model, e, &snippet)?),
            }
        }
        EditOp::AddSpecialization { target, general } => {
            let e = find(model, target)?;
            // `~` conjugates the type of a port; a general type takes none, and `:> ~A` is no SysML v2.
            if general.trim_start().starts_with('~') {
                return Err(format!("general {} starts with '~', which conjugates the type of a port and has no meaning for a general type. Fix: pass the name without '~'.", crate::agent::text::quote_clip(general.trim())));
            }
            let general = &qualified_text("general", general, false)?;
            match e.refs_of(RefRole::Specializes).last() {
                Some(r) => {
                    patches.push(Patch { file: e.file, span: Span::new(r.span.end, r.span.end), text: format!(", {general}") })
                }
                None => {
                    let at = e.name_span.ok_or("element has no name")?.end;
                    patches.push(Patch { file: e.file, span: Span::new(at, at), text: format!(" :> {general}") });
                }
            }
        }
        EditOp::SetType { target, type_ref } => {
            let e = find(model, target)?;
            if type_ref.trim().is_empty() {
                return Err("type is empty".into());
            }
            match e.refs_of(RefRole::Type).next() {
                Some(r) => {
                    // The `~` of a conjugated type is part of the type text to write, not of the reference span.
                    let text = file_text(model, e.file)?;
                    let start = if r.conjugated && upto(text, r.span.start)?.ends_with('~') { r.span.start - 1 } else { r.span.start };
                    patches.push(Patch { file: e.file, span: Span::new(start, r.span.end), text: type_ref.trim().to_string() })
                }
                None => {
                    let at = tail_end(e)?;
                    patches.push(Patch { file: e.file, span: Span::new(at, at), text: format!(" : {}", type_ref.trim()) });
                }
            }
        }
        EditOp::SetMultiplicity { target, multiplicity } => {
            let e = find(model, target)?;
            let m = multiplicity.trim().trim_start_matches('[').trim_end_matches(']').trim();
            let text = file_text(model, e.file)?;
            match (multiplicity_span(text, e), m.is_empty()) {
                (Some(sp), true) => {
                    let start = if upto(text, sp.start)?.ends_with(' ') { sp.start - 1 } else { sp.start };
                    patches.push(Patch { file: e.file, span: Span::new(start, sp.end), text: String::new() });
                }
                (Some(sp), false) => patches.push(Patch { file: e.file, span: sp, text: format!("[{m}]") }),
                (None, true) => {}
                (None, false) => {
                    let at = tail_end(e)?;
                    patches.push(Patch { file: e.file, span: Span::new(at, at), text: format!(" [{m}]") });
                }
            }
        }
        EditOp::SetValue { target, value } => {
            let e = find(model, target)?;
            let text = file_text(model, e.file)?;
            let v = value.trim();
            match (e.value_span, v.is_empty()) {
                (Some(vs), false) => {
                    // A space beside a `/` or `*` of the text around the value, so no comment marker can form there.
                    let lead = if text.get(..vs.start).is_some_and(|t| t.ends_with('/')) { " " } else { "" };
                    let trail = if text.get(vs.end..).is_some_and(|t| t.starts_with(['/', '*'])) { " " } else { "" };
                    patches.push(Patch { file: e.file, span: vs, text: format!("{lead}{v}{trail}") })
                }
                (Some(vs), true) => {
                    let eq = operator_before(text, vs.start, &[":=", "="]).ok_or("cannot find the '=' before the value")?;
                    let start = operator_before(text, eq, &["default"]).unwrap_or(eq);
                    patches.push(Patch { file: e.file, span: Span::new(start, vs.end), text: String::new() });
                }
                (None, false) => {
                    let at = head_end(text, e)?;
                    let trail = if text.get(at..).is_some_and(|t| t.starts_with(['/', '*'])) { " " } else { "" };
                    patches.push(Patch { file: e.file, span: Span::new(at, at), text: format!(" = {v}{trail}") });
                }
                (None, true) => {}
            }
        }
        EditOp::SetShortName { target, short } => {
            let e = find(model, target)?;
            let name = e.name_span.filter(|_| !e.implied_name).ok_or("element has no declared name")?;
            let text = file_text(model, e.file)?;
            let region = sub(text, e.span.start, name.start)?;
            let old = short_name_span(region).map(|r| Span::new(e.span.start + r.start, e.span.start + r.end));
            let s = short.trim();
            if !s.is_empty() {
                checked_name("short name", s)?;
            }
            // A basic name is written as it is (`<ek7m2qd>`); any other in quotes, a quote or a backslash in it escaped.
            let written = crate::model::quote_name(s);
            match (old, s.is_empty()) {
                (Some(sp), false) => patches.push(Patch { file: e.file, span: sp, text: format!("<{written}>") }),
                (Some(sp), true) => {
                    let end = if text.get(sp.end..).ok_or(STALE)?.starts_with(' ') { sp.end + 1 } else { sp.end };
                    patches.push(Patch { file: e.file, span: Span::new(sp.start, end), text: String::new() });
                }
                (None, false) => patches.push(Patch { file: e.file, span: Span::new(name.start, name.start), text: format!("<{written}> ") }),
                (None, true) => {}
            }
            // A reference written by the old id follows it, as a reference written by the name follows a rename.
            if let Some(was) = e.short_name.as_deref().filter(|w| !s.is_empty() && *w != s && Some(*w) != e.name.as_deref()) {
                for other in &model.elements {
                    for r in &other.refs {
                        for seg in r.segments.iter().filter(|g| g.resolved == Some(e.id) && g.name == was) {
                            patches.push(Patch { file: other.file, span: seg.span, text: written.clone() });
                        }
                    }
                }
            }
        }
        EditOp::RemoveSpecialization { target, index } => {
            let e = find(model, target)?;
            let refs: Vec<&Reference> = e.refs_of(RefRole::Specializes).collect();
            let r = refs.get(*index).ok_or("no such specialization")?;
            let text = file_text(model, e.file)?;
            let span = if refs.len() == 1 {
                let start = operator_before(text, r.span.start, &[":>", "subsets", "specializes"]).ok_or("cannot find the ':>' before the name")?;
                Span::new(start, r.span.end)
            } else if *index > 0 {
                Span::new(refs[index - 1].span.end, r.span.end)
            } else {
                Span::new(r.span.start, refs[1].span.start)
            };
            patches.push(Patch { file: e.file, span, text: String::new() });
        }
        EditOp::SetRedefines { target, names } => {
            let e = find(model, target)?;
            let text = file_text(model, e.file)?;
            let have: Vec<&Reference> = e.refs_of(RefRole::Redefines).collect();
            match (have.first(), names.is_empty()) {
                (Some(first), false) => patches.push(Patch { file: e.file, span: Span::new(first.span.start, have[have.len() - 1].span.end), text: names.join(", ") }),
                (Some(first), true) => {
                    let start = operator_before(text, first.span.start, &[":>>", "redefines"]).ok_or("cannot find the ':>>' before the name")?;
                    patches.push(Patch { file: e.file, span: Span::new(start, have[have.len() - 1].span.end), text: String::new() });
                }
                (None, false) => {
                    let at = tail_end(e)?;
                    patches.push(Patch { file: e.file, span: Span::new(at, at), text: format!(" :>> {}", names.join(", ")) });
                }
                (None, true) => {}
            }
        }
        EditOp::SetReferences { target, names } => {
            let e = find(model, target)?;
            let text = file_text(model, e.file)?;
            let have: Vec<&Reference> = e.refs_of(RefRole::References).collect();
            match (have.first(), names.is_empty()) {
                (Some(first), false) => patches.push(Patch { file: e.file, span: Span::new(first.span.start, have[have.len() - 1].span.end), text: names.join(", ") }),
                (Some(first), true) => {
                    let start = operator_before(text, first.span.start, &["::>", "references"]).ok_or("cannot find the '::>' before the name")?;
                    patches.push(Patch { file: e.file, span: Span::new(start, have[have.len() - 1].span.end), text: String::new() });
                }
                (None, false) => {
                    let at = tail_end(e)?;
                    patches.push(Patch { file: e.file, span: Span::new(at, at), text: format!(" ::> {}", names.join(", ")) });
                }
                (None, true) => {}
            }
        }
        EditOp::SetMetadata { target, tags } => {
            let e = find(model, target)?;
            let text = file_text(model, e.file)?;
            let stmt = sub(text, e.span.start, e.span.end)?;
            let (at, _, dir, words) = scan_prefixes(stmt);
            let new: String = tags.iter().map(|t| format!("#{t} ")).collect();
            patches.push(Patch { file: e.file, span: Span::new(e.span.start, e.span.start + at), text: head_text(&words, &new, dir.as_deref()) });
        }
        EditOp::SetPrefixes { target, direction, words } => {
            let e = find(model, target)?;
            let text = file_text(model, e.file)?;
            let stmt = sub(text, e.span.start, e.span.end)?;
            if e.props.contains_key("end") && words.is_some() {
                return Err("this end is written `end name [m] kind ...`; change its modifiers with edit_model replace".into());
            }
            let (at, metas, old_dir, old_words) = scan_prefixes(stmt);
            let first_kw = e.kind.split(' ').next().unwrap_or("");
            let keyword_next = ident_at(stmt, at) == first_kw;
            let keep = |w: &String| !TAIL_MODIFIERS.contains(&w.as_str());
            let lead: Vec<String> = match words {
                Some(w) => w.iter().filter(|x| keep(x)).cloned().collect(),
                None => old_words.clone(),
            };
            let new_dir = match direction.as_deref() {
                Some("") => None,
                Some(d) => Some(d.to_string()),
                None => old_dir.clone(),
            };
            let mut lead = lead;
            lead.sort_by_key(|w| modifier_rank(w));
            let head = head_text(&lead, &metas, new_dir.as_deref());
            let old_head = &stmt[..at];
            if head.trim() != old_head.trim() {
                // A statement such as `ref x : T;` or `in x : T;` has no keyword: its prefix word is its kind.
                let mut now = lead.clone();
                now.sort();
                let mut was = old_words.clone();
                was.sort();
                if !keyword_next && (now != was || new_dir.is_none()) {
                    return Err(format!("'{}' is written without a keyword, so its prefix words are its kind. Change it with edit_model replace.", e.qualified_name));
                }
                patches.push(Patch { file: e.file, span: Span::new(e.span.start, e.span.start + at), text: head });
            }
            if let Some(w) = words {
                let want: Vec<&str> = {
                    let mut v: Vec<&str> = w.iter().map(String::as_str).filter(|x| TAIL_MODIFIERS.contains(x)).collect();
                    v.sort_by_key(|x| modifier_rank(x));
                    v.dedup();
                    v
                };
                let (found, head_end) = tail_words(text, e)?;
                let mut have: Vec<&str> = found.iter().map(|(_, w)| w.as_str()).collect();
                have.sort_by_key(|x| modifier_rank(x));
                if have != want {
                    for (sp, _) in &found {
                        patches.push(Patch { file: e.file, span: *sp, text: String::new() });
                    }
                    let name_end = e.name_span.map(|n| n.end).unwrap_or(head_end);
                    let after_type = e.refs_of(RefRole::Type).filter(|r| r.span.start >= name_end).map(|r| r.span.end).fold(name_end, usize::max);
                    let pos = multiplicity_span(text, e).map(|m| m.end).unwrap_or(0).max(after_type);
                    let (mid, last): (Vec<&str>, Vec<&str>) = want.iter().partition(|x| **x != "parallel");
                    if !mid.is_empty() {
                        patches.push(Patch { file: e.file, span: Span::new(pos, pos), text: format!(" {}", mid.join(" ")) });
                    }
                    if !last.is_empty() {
                        // The grammar gives a parallel state a braced body, never `;`.
                        let semi = text.get(head_end..e.span.end.min(text.len())).and_then(|t| t.trim_start().strip_prefix(';').map(|_| t.len() - t.trim_start().len() + 1));
                        let (span, add) = match semi {
                            Some(n) => (Span::new(head_end, head_end + n), " parallel {}"),
                            None => (Span::new(head_end, head_end), " parallel"),
                        };
                        patches.push(Patch { file: e.file, span, text: add.into() });
                    }
                }
            }
        }
        EditOp::ClearType { target } => {
            let e = find(model, target)?;
            if let Some(r) = e.refs_of(RefRole::Type).next() {
                let text = file_text(model, e.file)?;
                let start = operator_before(text, r.span.start, &[":", "defined by", "typed by"]).ok_or("cannot find the ':' before the type")?;
                patches.push(Patch { file: e.file, span: Span::new(start, r.span.end), text: String::new() });
            }
        }
    }

    write_patches(model, patches, new_file)
}

/// The text a delete removes: the element, and its whole line when nothing else is on it.
fn delete_patch(model: &Model, e: &Element) -> Result<Patch, String> {
    let text = file_text(model, e.file)?;
    let line_start = upto(text, e.span.start)?.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let start = if sub(text, line_start, e.span.start)?.trim().is_empty() { line_start } else { e.span.start };
    let rest = text.get(e.span.end..).ok_or(STALE)?;
    let ws = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let mut end = e.span.end + ws;
    let after = text.get(end..).ok_or(STALE)?;
    if after.starts_with("\r\n") {
        end += 2;
    } else if after.starts_with('\n') {
        end += 1;
    } else {
        end = e.span.end;
    }
    Ok(Patch { file: e.file, span: Span::new(start, end), text: String::new() })
}

/// Delete several elements in one pass over the text, with the result of deleting them one by one from the last
/// to the first. None of them may be inside another.
pub fn delete_many(model: &Model, targets: &[&str]) -> Result<Vec<FileChange>, String> {
    let mut patches = Vec::with_capacity(targets.len());
    for t in targets {
        patches.push(delete_patch(model, find(model, t)?)?);
    }
    write_patches(model, patches, None)
}

/// True when `left` then `right` side by side would open a note: `/` before `/` or `*`.
fn joins_note(left: Option<char>, right: Option<char>) -> bool {
    left == Some('/') && matches!(right, Some('/' | '*'))
}

fn write_patches(model: &Model, patches: Vec<Patch>, new_file: Option<FileChange>) -> Result<Vec<FileChange>, String> {
    // Bound the work before doing it: the new text of all patches, and of a new file.
    let added: usize = patches.iter().map(|p| p.text.len()).sum::<usize>() + new_file.as_ref().map(|f| f.text.len()).unwrap_or(0);
    if added > MAX_ADDED_BYTES {
        return Err(format!("refused: the edit writes {added} bytes of new text, which is larger than the {} MiB one edit may add", MAX_ADDED_BYTES >> 20));
    }
    let mut out = Vec::new();
    for (i, f) in model.files.iter().enumerate() {
        let mut mine: Vec<&Patch> = patches.iter().filter(|p| p.file == i).collect();
        if mine.is_empty() {
            continue;
        }
        // Left to right. Two changes at the same place count once; changes that overlap, or that do not fit
        // this text, mean the spans came from other text, and nothing is written.
        mine.sort_by_key(|p| (p.span.start, p.span.end));
        mine.dedup_by(|a, b| a.span == b.span);
        let mut text = String::with_capacity(f.text.len() + added);
        let mut at = 0;
        for p in mine {
            if p.span.start < at || p.span.end < p.span.start {
                return Err("refused: two changes of the edit overlap".into());
            }
            text.push_str(sub(&f.text, at, p.span.start)?);
            // New text must not make a comment marker with the text beside it: a value that ends in `/` before an
            // existing `/* note */` would read as a line note and swallow the rest of the line, `;` included.
            if joins_note(text.chars().next_back(), p.text.chars().next()) {
                text.push(' ');
            }
            text.push_str(&p.text);
            // The range itself must be inside the text and between characters.
            sub(&f.text, p.span.start, p.span.end)?;
            at = p.span.end;
            let after = f.text.get(at..).ok_or(STALE)?;
            if joins_note(text.chars().next_back(), after.chars().next()) {
                text.push(' ');
            }
        }
        text.push_str(f.text.get(at..).ok_or(STALE)?);
        out.push(FileChange { path: f.path.clone(), text });
    }
    out.extend(new_file);
    Ok(out)
}
