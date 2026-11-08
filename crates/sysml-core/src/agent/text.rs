//! Text helpers shared by the tools: names a person can read, CSV and Markdown output, quoting of model and caller
//! text, units of attributes, requirement ids, and the member walk of an element.

use super::graph;
use super::units;
use super::{line_of, qshow};
use crate::model::*;
use std::cmp::Ordering;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet};

// ---- names a person can read ------------------------------------------------------------------------------------------

/// Names as a reader sees them: without the one root package every name starts with, and with a statement that has no
/// name (`Pkg::Owner::$19`) described by what it joins.
pub struct Names<'a> {
    m: &'a Model,
    root: Option<String>,
    /// Short names for a document a person reads: an element is its name with its owner (`swapStatus in
    /// SwapStatusPort`), and a path inside a sentence is its last name. The full path is in the structured reply.
    brief: bool,
}

fn is_named(e: &Element) -> bool {
    e.name.is_some()
}

impl<'a> Names<'a> {
    pub fn new(m: &'a Model) -> Self {
        let packages: Vec<&Element> = m.roots.iter().map(|r| &m.elements[*r]).filter(|e| e.kind == "package").collect();
        let root = if packages.len() == 1 { Some(format!("{}::", packages[0].qualified_name)) } else { None };
        Names { m, root, brief: false }
    }

    /// The names for a document: see `brief`.
    pub fn brief(m: &'a Model) -> Self {
        Names { brief: true, ..Names::new(m) }
    }

    /// The last name of a path (`Pkg::Port::status` gives `status`); a feature chain written with dots is kept.
    fn last(qn: &str) -> String {
        let (mut cut, mut quoted) = (0usize, false);
        let b = qn.as_bytes();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\\' if quoted => i += 1,
                b'\'' => quoted = !quoted,
                b':' if !quoted && b.get(i + 1) == Some(&b':') => {
                    cut = i + 2;
                    i += 1;
                }
                _ => {}
            }
            i += 1;
        }
        qn.get(cut..).unwrap_or(qn).to_string()
    }

    /// A qualified name, or the text of a reference, without the root package.
    pub fn rel(&self, qn: &str) -> String {
        if self.brief {
            return Self::last(qn);
        }
        match &self.root {
            Some(r) if qn.starts_with(r.as_str()) && qn.len() > r.len() => qn[r.len()..].to_string(),
            _ => qn.to_string(),
        }
    }

    fn owner_text(&self, e: &Element) -> String {
        match e.owner.map(|o| &self.m.elements[o]) {
            Some(o) if o.kind == "objective" => self.owner_text(o),
            Some(o) if self.brief && o.name.is_some() && !o.implied_name => o.display_name(),
            Some(o) => self.show(o),
            None => "the model".into(),
        }
    }

    /// What a reader calls the element: its name, or for a statement what it says.
    pub fn show(&self, e: &Element) -> String {
        let m = self.m;
        let text = |role: RefRole| e.refs_of(role).next().map(|r| self.rel(&r.text));
        let ends = || {
            let s = graph::statements_of(m, e.id);
            let end = |x: &graph::End| x.id.map(|i| self.show_named(&m.elements[i])).unwrap_or_else(|| self.rel(&x.text));
            s.first().map(|s| (end(&s.source), end(&s.target)))
        };
        let written = || (text(RefRole::Source).unwrap_or_else(|| "?".into()), text(RefRole::Target).unwrap_or_else(|| "?".into()));
        let k = e.kind.as_str();
        if k.starts_with("satisfy") && !e.is_def {
            return match ends() {
                Some((s, t)) => format!("satisfy {t} by {s}"),
                None => format!("satisfy (in {})", self.owner_text(e)),
            };
        }
        if k.starts_with("verify") && !e.is_def {
            return match ends() {
                Some((s, t)) => format!("verify {t} by {s}"),
                None => format!("verify (in {})", self.owner_text(e)),
            };
        }
        if k.starts_with("perform") {
            let what = e.refs.iter().find(|r| matches!(r.role, RefRole::Target | RefRole::Type | RefRole::Specializes | RefRole::References)).map(|r| self.rel(&r.text)).or_else(|| e.name.clone()).unwrap_or_else(|| "?".into());
            return format!("{} performs {what}", self.owner_text(e));
        }
        if e.is_def {
            return self.show_named(e);
        }
        match k {
            "flow" | "message" => {
                let (s, t) = written();
                let of = text(RefRole::Type).map(|x| format!(" of {x}")).unwrap_or_default();
                let name = e.name.as_ref().filter(|_| !e.implied_name).map(|n| format!(" {n}")).unwrap_or_default();
                format!("{k}{name}{of} from {s} to {t}")
            }
            "connection" | "interface" | "binding" if !is_named(e) || e.implied_name => {
                let (s, t) = written();
                format!("{k} {s} to {t} (in {})", self.owner_text(e))
            }
            "succession" => {
                let (s, t) = written();
                format!("succession {s} then {t} (in {})", self.owner_text(e))
            }
            "transition" if !is_named(e) => {
                let (s, t) = written();
                format!("transition {s} to {t} (in {})", self.owner_text(e))
            }
            "allocation" if !is_named(e) => {
                let (s, t) = written();
                format!("allocate {s} to {t}")
            }
            "dependency" => {
                let tag = e.prefixes.iter().find_map(|p| p.strip_prefix('#')).map(|t| format!("{} ", t.rsplit("::").next().unwrap_or(t))).unwrap_or_default();
                let (s, t) = written();
                let name = e.name.as_ref().map(|n| format!(" '{n}'")).unwrap_or_default();
                format!("{tag}dependency{name} from {s} to {t}")
            }
            "objective" => format!("objective of {}", self.owner_text(e)),
            "metadata" => format!("@{} on {}", text(RefRole::Type).unwrap_or_else(|| "metadata".into()), self.owner_text(e)),
            "import" => format!("import {} (in {})", e.refs.first().map(|r| self.rel(&r.text)).unwrap_or_default(), self.owner_text(e)),
            _ => self.show_named(e),
        }
    }

    /// A sentence that names elements by their qualified names, as a reader sees it: each name without the root
    /// package, and the name of a statement that has none (`Pkg::Owner::$19`) replaced by what the statement says.
    pub fn rewrite(&self, text: &str) -> String {
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
        // The end (a char index) of the name segment that starts at char index `i`.
        let segment = |i: usize| -> Option<usize> {
            match chars.get(i)?.1 {
                '\'' => {
                    let mut j = i + 1;
                    while j < chars.len() {
                        match chars[j].1 {
                            '\\' => j += 2,
                            '\'' => return Some(j + 1),
                            '\n' => return None,
                            _ => j += 1,
                        }
                    }
                    None
                }
                c if word(c) => Some((i..chars.len()).find(|j| !word(chars[*j].1)).unwrap_or(chars.len())),
                _ => None,
            }
        };
        let byte = |i: usize| chars.get(i).map(|c| c.0).unwrap_or(text.len());
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            let starts = i == 0 || !(word(chars[i - 1].1) || chars[i - 1].1 == ':' || chars[i - 1].1 == '\'');
            let Some(mut end) = segment(i).filter(|_| starts) else {
                out.push(chars[i].1);
                i += 1;
                continue;
            };
            let mut segments = 1;
            while chars.get(end).map(|c| c.1) == Some(':') && chars.get(end + 1).map(|c| c.1) == Some(':') {
                match segment(end + 2) {
                    Some(e) => {
                        end = e;
                        segments += 1;
                    }
                    None => break,
                }
            }
            let token = &text[byte(i)..byte(end)];
            if segments < 2 {
                out.push_str(token);
            } else {
                match self.m.find(token) {
                    Some(e) if has_number_segment(&e.qualified_name) => {
                        // "flow <flow of X from a to b>" says the kind twice.
                        let shown = self.show(e);
                        let first = shown.split(' ').next().unwrap_or("");
                        if !first.is_empty() && out.ends_with(&format!("{first} ")) {
                            out.truncate(out.len() - first.len() - 1);
                        }
                        out.push_str(&shown);
                    }
                    _ => out.push_str(&self.rel(token)),
                }
            }
            i = end;
        }
        out
    }

    /// The path of a named element; when an owner on the way has no name, the name and what it is in.
    fn show_named(&self, e: &Element) -> String {
        if self.brief {
            if let Some(n) = e.name.as_ref().filter(|_| !has_number_segment(Self::last(&e.qualified_name).as_str())) {
                // The owner once, unless it is the root package every name of the model starts with.
                let owner = e.owner.map(|o| &self.m.elements[o]).filter(|o| self.root.as_deref() != Some(format!("{}::", o.qualified_name).as_str()));
                return match owner {
                    Some(_) => format!("{n} in {}", self.owner_text(e)),
                    None => n.clone(),
                };
            }
        }
        let path = self.rel(&e.qualified_name);
        if !has_number_segment(&path) {
            return path;
        }
        match &e.name {
            Some(n) => format!("{n} (in {})", self.owner_text(e)),
            None => format!("{} in {}", e.keyword(), self.owner_text(e)),
        }
    }
}

/// True when a qualified name has a segment the parser numbered (`$3`): the name of a statement that has none.
pub fn has_number_segment(qn: &str) -> bool {
    qn.split("::").any(|s| s.len() > 1 && s.starts_with('$') && s[1..].chars().all(|c| c.is_ascii_digit()))
}

// ---- csv, text and markdown tables ------------------------------------------------------------------------------------

/// Is the first character that is not white space one a spreadsheet reads as the start of a formula: `=`, `+`, `-`, `@`, their
/// full-width forms, the minus sign, or a tab or carriage return anywhere in front?
pub fn starts_a_formula(s: &str) -> bool {
    for c in s.chars() {
        match c {
            '\t' | '\r' => return true,
            '=' | '+' | '-' | '@' | '\u{ff1d}' | '\u{ff0b}' | '\u{ff0d}' | '\u{ff20}' | '\u{2212}' => return true,
            c if c.is_whitespace() || matches!(c, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' | '\u{2060}') => continue,
            _ => return false,
        }
    }
    false
}

/// The one guard of every CSV the tools write: a field that starts a formula gets a single quote in front, which a spreadsheet
/// shows as text.
pub fn formula_safe(s: &str) -> String {
    if starts_a_formula(s) {
        format!("'{s}")
    } else {
        s.to_string()
    }
}

/// The guard taken off what `formula_safe` put on: a leading single quote in front of a field that would start a formula.
pub fn strip_formula_guard(s: &str) -> &str {
    match s.strip_prefix('\'') {
        Some(rest) if starts_a_formula(rest) => rest,
        _ => s,
    }
}

/// One CSV field (RFC 4180): quoted when it holds a comma, a quote or a line break (or always, with `always`); a quote is doubled.
///
/// A cell that starts a formula (see `starts_a_formula`) is run as one when the file is opened in a spreadsheet, and the text
/// comes from the model, which may come from an imported project. Such a cell gets a single quote in front. Every CSV the tools
/// return is written here.
pub fn csv_field_with(s: &str, always: bool) -> String {
    let s = formula_safe(s);
    if always || s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

pub fn csv_field(s: &str) -> String {
    csv_field_with(s, false)
}

/// CSV text with every field in quotes: the form a spreadsheet round trip is written in.
pub fn csv_all_quoted(headers: &[String], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    let mut line = |cells: &[String]| {
        out.push_str(&cells.iter().map(|c| csv_field_with(c, true)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    };
    line(headers);
    for r in rows {
        line(r);
    }
    out
}

/// CSV text: one header row, then the rows; lines end with CR LF.
pub fn csv(headers: &[String], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    let mut line = |cells: &[String]| {
        out.push_str(&cells.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    };
    line(headers);
    for r in rows {
        line(r);
    }
    out
}

fn clip(s: &str, n: usize) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}...", one.chars().take(n.saturating_sub(3)).collect::<String>())
    }
}

/// A table in plain text: columns padded to the widest cell (cells are cut at `width` characters).
pub fn text_table(headers: &[&str], rows: &[Vec<String>], width: usize) -> String {
    text_table_keeping(headers, rows, width, &[])
}

/// `text_table`, but the columns named in `whole` are never cut (a reader must see all of them).
pub fn text_table_keeping(headers: &[&str], rows: &[Vec<String>], width: usize, whole: &[&str]) -> String {
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            r.iter()
                .enumerate()
                .map(|(i, c)| if headers.get(i).map(|h| whole.contains(h)).unwrap_or(false) { c.split_whitespace().collect::<Vec<_>>().join(" ") } else { clip(c, width) })
                .collect()
        })
        .collect();
    let mut w: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for r in &cells {
        for (i, c) in r.iter().enumerate() {
            if i < w.len() {
                w[i] = w[i].max(c.chars().count());
            }
        }
    }
    let line = |r: Vec<&str>| r.iter().enumerate().map(|(i, c)| format!("{c:<width$}", width = w.get(i).copied().unwrap_or(0))).collect::<Vec<_>>().join(" | ").trim_end().to_string();
    let mut out = line(headers.to_vec());
    out.push('\n');
    out.push_str(&w.iter().map(|n| "-".repeat(*n)).collect::<Vec<_>>().join("-+-"));
    out.push('\n');
    for r in &cells {
        out.push_str(&line(r.iter().map(String::as_str).collect()));
        out.push('\n');
    }
    out
}

/// Model text as it may stand in a Markdown document: on one line, and with no character that could end a table cell
/// or start markup. A backslash goes before `\\`, `|`, a backtick, `<`, `>`, `[`, `]`, `*`, `&`, and before a `_` or `~`
/// at the edge of a word and a `#` at the start (an underscore inside a word, as in `pad_sensor`, is plain text in
/// Markdown and stays as it is). Every Markdown document the tools return passes model text through here.
pub fn md_text(s: &str) -> String {
    let one: Vec<char> = s.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    let mut out = String::with_capacity(one.len() + 8);
    for (i, c) in one.iter().copied().enumerate() {
        let word = |j: Option<usize>| j.and_then(|j| one.get(j)).map(|x| x.is_alphanumeric()).unwrap_or(false);
        let escape = match c {
            '\\' | '|' | '`' | '<' | '>' | '[' | ']' | '*' | '&' => true,
            '_' | '~' => !(word(i.checked_sub(1)) && word(Some(i + 1))),
            '#' => i == 0,
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Text of a person with line breaks as it stands in a paragraph: every line escaped by `md_text`, the lines kept apart by
/// a hard break, a blank line kept as a paragraph break.
pub fn md_lines(s: &str) -> String {
    s.lines().map(|l| md_line_start(md_text(l))).collect::<Vec<_>>().join("  \n")
}

/// A line that stands at the start of a line of a document must not open a block: after `md_text` (which trims leading
/// white space, so no indented code block, and escapes `#`, `>`, `|`, `*`, a backtick, `~`, `_` and `<`) a leading `-`, `+` or `=`
/// (a list, a rule, a heading underline) is escaped, and so is the `.` or `)` that follows the digits of an ordered list.
fn md_line_start(l: String) -> String {
    let digits = l.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && matches!(l[digits..].chars().next(), Some('.' | ')')) {
        return format!("{}\\{}", &l[..digits], &l[digits..]);
    }
    if l.starts_with(['-', '+', '=']) {
        return format!("\\{l}");
    }
    l
}

/// A text that reached the tool from a person or an agent, as it may stand in a line of the tool's own: a character that
/// does not show, or that turns the direction or ends a line, is written as a visible escape (`{U+202E}`).
pub fn visible(s: &str) -> String {
    s.chars()
        .map(|c| if (c.is_control() && !matches!(c, '\n' | '\t' | '\r')) || crate::edit::is_invisible(c) { format!("{{U+{:04X}}}", c as u32) } else { c.to_string() })
        .collect()
}

/// A name or a short text somebody typed, as a claim: in double quotes, with `"` and `\` escaped and hidden characters
/// made visible, so that it cannot read as a part of the sentence it stands in ("signed as \"x at ...; assessment: complete\"").
pub fn quote_claim(s: &str) -> String {
    // A line break or a tab inside the quotes would end the line of the reply: they are written as escapes.
    // A typographic quote looks like the end of the claim to a reader, so it is shown as an escape too.
    format!("\"{}\"", visible(s).replace('\u{201c}', "{U+201C}").replace('\u{201d}', "{U+201D}").replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t"))
}

/// A claim cut to `crate::edit::clip` first: for a text of any length that stands in an error message.
pub fn quote_clip(s: &str) -> String {
    quote_claim(&crate::edit::clip(s))
}

/// A time, a revision, an id or a number read back from a file of the workspace, in a line of a reply: only the characters
/// such a value has, so that a file somebody edited cannot put words into the line. A string, a number or a boolean is read;
/// anything else, or nothing left, is `?`.
pub fn stored_token(v: &serde_json::Value) -> String {
    let raw = match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => String::new(),
    };
    let s: String = raw.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '-' | '#' | '+' | '_')).take(40).collect();
    if s.is_empty() {
        "?".into()
    } else {
        s
    }
}

/// A name of the model (an element, a title) in a line of a reply: hidden characters and line breaks are written as visible
/// escapes. The name is not a claim of a person, so it is not quoted.
pub fn model_name(s: &str) -> String {
    visible(s).replace('\n', "{LF}").replace('\r', "{CR}").replace('\t', "{TAB}")
}

/// Words that an agent or a person wrote over several lines, in a block of their own: a block quote, headed by a line that
/// says whose words they are, with every line prefixed by the mark \u{BB} and escaped by `md_text`. No line of such a text can
/// look like a line, a table row, a heading or a tag of the tool or the server. `who` is the tool's own wording.
pub fn md_quote(who: &str, s: &str) -> String {
    let mut out = format!("> **{}**: quoted words. The tool and the server did not write the lines below; each starts with \u{BB}.\n>\n", md_text(who));
    let body = s.replace("\r\n", "\n").replace('\r', "\n");
    for l in body.split('\n') {
        let e = md_text(&visible(l));
        out.push_str(&if e.is_empty() { "> \u{BB}\n".to_string() } else { format!("> \u{BB} {e}\n") });
    }
    out.push('\n');
    out
}

/// One cell of a Markdown table: text made safe by `visible` and `md_text`, always one line. A line break or a line separator
/// in the text is a space, and a character that does not show (a vertical tab, a direction mark, a zero-width character) is
/// written as a visible escape, so no text can open a second line in a cell or put a tag of its own in one. An empty cell
/// shows a dash. (A text with lines in it is quoted by `md_quote` below the table, never carried in a cell.)
pub fn md_cell(s: &str) -> String {
    let one = md_text(&visible(s));
    if one.is_empty() {
        "-".into()
    } else {
        one
    }
}

/// A Markdown table. A table without rows is not written: the caller says in a sentence that there is nothing.
pub fn md_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut out = format!("| {} |\n|{}|\n", headers.join(" | "), headers.iter().map(|_| "---").collect::<Vec<_>>().join("|"));
    for r in rows {
        out.push_str(&format!("| {} |\n", r.iter().map(|c| md_cell(c)).collect::<Vec<_>>().join(" | ")));
    }
    out
}

// ---- units ------------------------------------------------------------------------------------------------------------

/// The SI unit of a quantity type of the official library (`ISQ::DurationValue` is measured in seconds).
fn si_unit(type_name: &str) -> Option<&'static str> {
    Some(match type_name.rsplit("::").next().unwrap_or(type_name) {
        "DurationValue" | "TimeValue" => "s",
        "MassValue" => "kg",
        "LengthValue" | "DistanceValue" => "m",
        "SpeedValue" | "VelocityValue" => "m/s",
        "AccelerationValue" => "m/s^2",
        "ForceValue" => "N",
        "PowerValue" => "W",
        "EnergyValue" => "J",
        "ElectricPotentialValue" | "VoltageValue" => "V",
        "ElectricCurrentValue" => "A",
        "ResistanceValue" | "ElectricResistanceValue" => "ohm",
        "ThermodynamicTemperatureValue" | "TemperatureValue" => "K",
        "FrequencyValue" => "Hz",
        "PressureValue" => "Pa",
        "AngularMeasureValue" | "PlaneAngleValue" => "rad",
        "AreaValue" => "m^2",
        "VolumeValue" => "m^3",
        "TorqueValue" => "N*m",
        "MassFlowRateValue" => "kg/s",
        _ => return None,
    })
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_string()
}

/// The unit of an attribute and where it is stated: the unit in brackets after its value (`= 45 [s]`), else the unit its
/// value type names (`@ValueType { unit = Units::Second; }`, as an imported project writes it), else the SI unit of
/// its quantity type (`ISQ::DurationValue`).
pub fn unit_of(m: &Model, attr: &Element) -> Option<(String, &'static str)> {
    if let Some(v) = &attr.value {
        let names = units::unit_names(v);
        if !names.is_empty() {
            return Some((names.join(" "), "value"));
        }
    }
    let mut seen = BTreeSet::new();
    let mut at = attr.first_resolved(RefRole::Type);
    while let Some(t) = at.filter(|t| seen.insert(*t) && seen.len() <= 8) {
        let te = &m.elements[t];
        for meta in m.children(t).filter(|c| c.kind == "metadata") {
            if let Some(u) = m.children(meta.id).find(|c| c.name.as_deref().map(|n| n.eq_ignore_ascii_case("unit")).unwrap_or(false)).and_then(|c| c.value.clone()) {
                let u = unquote(&u);
                return Some((u.rsplit("::").next().unwrap_or(&u).trim_matches('\'').to_string(), "value type"));
            }
        }
        if let Some(u) = si_unit(&te.qualified_name) {
            return Some((u.to_string(), "quantity type"));
        }
        at = te.first_resolved(RefRole::Specializes);
    }
    attr.refs_of(RefRole::Type).find_map(|r| si_unit(&r.text)).map(|u| (u.to_string(), "quantity type"))
}

// ---- small model readers ----------------------------------------------------------------------------------------------

/// The requirement id of an element: its short name, else the value of an attribute called id.
pub(crate) fn id_of(m: &Model, e: &Element) -> String {
    if let Some(s) = &e.short_name {
        return s.clone();
    }
    e.children.iter().map(|c| &m.elements[*c]).find(|c| c.kind == "attribute" && c.name.as_deref().map(|n| n.eq_ignore_ascii_case("id") || n.eq_ignore_ascii_case("reqid")).unwrap_or(false)).and_then(|c| c.value.clone()).map(|v| v.trim().trim_matches('"').to_string()).unwrap_or_default()
}

/// Natural order: runs of digits compare as numbers, so 1.2 comes before 1.10.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (si, sj) = (i, j);
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let (x, y): (String, String) = (a[si..i].iter().collect::<String>().trim_start_matches('0').to_string(), b[sj..j].iter().collect::<String>().trim_start_matches('0').to_string());
            let o = x.len().cmp(&y.len()).then_with(|| x.cmp(&y));
            if o != Ordering::Equal {
                return o;
            }
        } else {
            let (x, y) = (a[i].to_lowercase().next().unwrap_or(a[i]), b[j].to_lowercase().next().unwrap_or(b[j]));
            if x != y {
                return x.cmp(&y);
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

/// The text of an element's documentation: its own doc, else the doc comment it owns, trimmed; None when empty.
pub(crate) fn doc_of(m: &Model, e: &Element) -> Option<String> {
    e.doc.clone().or_else(|| m.children(e.id).find(|c| c.kind == "doc").and_then(|c| c.doc.clone())).map(|d| d.trim().to_string()).filter(|d| !d.is_empty())
}

/// The values an element states: its attributes that have a value.
pub(crate) fn values(m: &Model, e: &Element) -> Vec<(String, String, ElementId)> {
    m.children(e.id).filter(|c| c.kind == "attribute" && !c.is_def).filter_map(|c| Some((c.name.clone()?, c.value.clone()?.trim().to_string(), c.id))).collect()
}

const RELATIONSHIP_KINDS: &[&str] = &[
    "connection", "binding", "succession", "flow", "transition", "dependency", "allocation", "satisfy", "verify",
    "import", "expose", "alias", "comment", "doc", "expression", "metadata", "filter", "message",
];

/// Own members followed by inherited ones. Redefined names are not repeated.
pub fn all_members(model: &Model, el: ElementId) -> Vec<ElementId> {
    fn walk(model: &Model, el: ElementId, seen: &mut HashSet<ElementId>, names: &mut HashSet<String>, out: &mut Vec<ElementId>) {
        if !seen.insert(el) || seen.len() > 64 {
            return;
        }
        let e = &model.elements[el];
        for c in &e.children {
            let ce = &model.elements[*c];
            if let Some(n) = &ce.name {
                if !names.insert(format!("{}\u{0}{}", n, group(&ce.kind))) {
                    continue;
                }
            }
            out.push(*c);
        }
        for r in &e.refs {
            let follow = matches!(r.role, RefRole::Type | RefRole::Specializes | RefRole::Redefines)
                || (r.role == RefRole::Target && matches!(e.kind.as_str(), "perform" | "exhibit"));
            if follow {
                if let Some(t) = r.resolved() {
                    walk(model, t, seen, names, out);
                }
            }
        }
    }
    fn group(kind: &str) -> &str {
        if RELATIONSHIP_KINDS.contains(&kind) {
            "rel"
        } else {
            "feature"
        }
    }
    let mut out = Vec::new();
    walk(model, el, &mut HashSet::new(), &mut HashSet::new(), &mut out);
    out
}

// ---- elements of one name in two places -----------------------------------------------------------------------------
/// True for the kinds of which a reader expects one of a name in a model: a definition, and a requirement.
fn twin_kind(e: &Element) -> bool {
    !e.implied_name && e.name.is_some() && (e.is_def || e.kind == "requirement") && e.kind != "package"
}

/// For each element of `refs` that has a twin (an element of the same kind and the same name in another owner): one
/// note that names the twins and where each is. `other` ends the sentence for a twin that is not in `refs` ("it is not
/// on this list", "not changed").
pub(crate) fn twin_notes(m: &Model, refs: &[String], other: &str) -> Vec<Value> {
    let mine = refs.iter().filter_map(|q| m.find(q)).any(twin_kind);
    if !mine {
        return Vec::new();
    }
    twin_notes_in(m, &twin_index(m), refs, other)
}

/// The `twin_kind` elements of a model by (kind, definition or not, name).
type TwinIndex = HashMap<(String, bool, String), Vec<ElementId>>;

fn twin_index(m: &Model) -> TwinIndex {
    let mut by_name: TwinIndex = HashMap::new();
    for e in m.elements.iter().filter(|e| twin_kind(e)) {
        by_name.entry((e.kind.clone(), e.is_def, e.name.clone().unwrap_or_default())).or_default().push(e.id);
    }
    by_name
}

fn twin_notes_in(m: &Model, index: &TwinIndex, refs: &[String], other: &str) -> Vec<Value> {
    let mine: Vec<&Element> = {
        let mut seen = BTreeSet::new();
        refs.iter().filter_map(|q| m.find(q)).filter(|e| twin_kind(e) && seen.insert(e.id)).collect()
    };
    if mine.is_empty() {
        return Vec::new();
    }
    let listed: BTreeSet<ElementId> = mine.iter().map(|e| e.id).collect();
    let mut out = Vec::new();
    let mut told: BTreeSet<(String, bool, String)> = BTreeSet::new();
    for e in mine {
        let key = (e.kind.clone(), e.is_def, e.name.clone().unwrap_or_default());
        let others: Vec<&Element> = index.get(&key).into_iter().flatten().map(|x| &m.elements[*x]).filter(|x| x.id != e.id && x.owner != e.owner).collect();
        if others.is_empty() || !told.insert((e.kind.clone(), e.is_def, e.name.clone().unwrap_or_default())) {
            continue;
        }
        let owner_of = |x: &Element| x.owner.map(|o| m.elements[o].qualified_name.clone()).unwrap_or_else(|| "the model".into());
        let what = if e.kind == "item" && e.is_def { "signal".to_string() } else { e.keyword() };
        let parts: Vec<String> = others.iter().take(5).map(|x| format!("{} ({}:{}){}", owner_of(x), m.files[x.file].path, line_of(m, x), if listed.contains(&x.id) { ", also here" } else { "" })).collect();
        let all_listed = others.iter().all(|x| listed.contains(&x.id));
        out.push(json!({
            "ref": e.qualified_name,
            "kind": e.keyword(),
            "name": e.name,
            "others": others.iter().map(|x| json!({ "ref": x.qualified_name, "owner": owner_of(x), "file": m.files[x.file].path, "line": line_of(m, x), "listed": listed.contains(&x.id) })).collect::<Vec<_>>(),
            "says": format!(
                "Another {what} named {} exists in {}{}{}. This one is {}.",
                qshow(e.name.as_deref().unwrap_or("")),
                parts.join("; in "),
                if others.len() > 5 { format!("; and {} more", others.len() - 5) } else { String::new() },
                if all_listed { String::new() } else { format!("; {other}") },
                e.qualified_name
            ),
        }));
    }
    out
}
