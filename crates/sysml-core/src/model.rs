//! In-memory element graph produced by the parser.
//!
//! Text is the source of truth. Every element keeps byte spans into its
//! source file so edits can be applied as text patches.

use serde::Serialize;
use std::collections::BTreeMap;

pub type ElementId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RefRole {
    Type,
    Specializes,
    Redefines,
    References,
    Source,
    Target,
    Import,
    Other,
    /// A metadata prefix: the `Pkg::Tag` of `#Pkg::Tag part x;`.
    Meta,
    /// A name inside a value expression: `= Nope::Missing`, `= other.y`.
    Expr,
    /// The feature a bare assignment sets: `level` in `@M { level = L3; }`.
    Assign,
    /// The function of a call or a name in brackets inside a value, a guard, a trigger or an effect: `f` in `f(x)`, `i`
    /// in `a[i]`. It is a reference when it resolves; when it does not (a library function, a unit) nothing is reported.
    Call,
}

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub name: String,
    pub span: Span,
    pub resolved: Option<ElementId>,
}

/// A qualified name or feature chain that points at another element.
#[derive(Debug, Clone, Serialize)]
pub struct Reference {
    pub role: RefRole,
    pub text: String,
    pub span: Span,
    pub segments: Vec<Segment>,
    pub conjugated: bool,
}

impl Reference {
    pub fn resolved(&self) -> Option<ElementId> {
        self.segments.last().and_then(|s| s.resolved)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Element {
    pub id: ElementId,
    pub file: usize,
    /// Keyword phrase without `def`, for example `part`, `use case`, `package`.
    pub kind: String,
    pub is_def: bool,
    pub name: Option<String>,
    /// True when the name was taken from a redefined feature (`:>> mass`).
    pub implied_name: bool,
    pub short_name: Option<String>,
    pub qualified_name: String,
    pub owner: Option<ElementId>,
    pub children: Vec<ElementId>,
    pub span: Span,
    pub name_span: Option<Span>,
    /// Span of `{ ... }` including both braces.
    pub body_span: Option<Span>,
    pub prefixes: Vec<String>,
    pub direction: Option<String>,
    pub multiplicity: Option<String>,
    pub refs: Vec<Reference>,
    pub value: Option<String>,
    /// Where `value` sits in the file (text after `=`).
    pub value_span: Option<Span>,
    pub doc: Option<String>,
    pub props: BTreeMap<String, String>,
}

impl Element {
    pub fn has_prefix(&self, p: &str) -> bool {
        self.prefixes.iter().any(|x| x == p)
    }

    pub fn refs_of(&self, role: RefRole) -> impl Iterator<Item = &Reference> {
        self.refs.iter().filter(move |r| r.role == role)
    }

    pub fn first_resolved(&self, role: RefRole) -> Option<ElementId> {
        self.refs_of(role).find_map(|r| r.resolved())
    }

    /// Keyword phrase as written, for example `part def`.
    pub fn keyword(&self) -> String {
        if self.is_def {
            format!("{} def", self.kind)
        } else {
            self.kind.clone()
        }
    }

    pub fn display_name(&self) -> String {
        self.name.clone().unwrap_or_else(|| format!("<{}>", self.kind))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub file: usize,
    pub span: Span,
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Default, Clone)]
pub struct Model {
    pub files: Vec<SourceFile>,
    pub elements: Vec<Element>,
    pub roots: Vec<ElementId>,
    pub diagnostics: Vec<Diagnostic>,
    /// How many of `diagnostics` the parser gave (they come first): the parse faults, which a write reply lists first.
    pub parsed_diagnostics: usize,
    /// The workspace setting `budget_root` (a part, by name), set by the host; it pins the root of an unrooted budget.
    pub budget_root: Option<String>,
    /// The source ids (from an import) and the ids the tools set, by id: the element each names. Set by the host; a reference in text
    /// `(element <id>)` is read through it.
    pub source_ids: std::collections::BTreeMap<String, String>,
    /// Byte offset of each line start, per file; built on the first `line_col` call. The files of a model do
    /// not change after it is analyzed.
    pub(crate) line_starts: std::sync::OnceLock<Vec<Vec<usize>>>,
    /// First element of each qualified name, built on the first `find`. The elements do not change after analysis.
    pub(crate) by_name: std::sync::OnceLock<std::collections::HashMap<String, ElementId>>,
    /// What `get_connections` reads (the connection index, the statements, the allocations), built on its first call.
    pub(crate) conn_facts: Memo<crate::agent::connections::Facts>,
    /// The duplicate groups' counts (`agent::duplicates`), built on the first write check that asks: a batch of edits
    /// compares each edit's model with the one before, which the edit before already counted.
    #[allow(clippy::type_complexity)]
    pub(crate) dup_counts: std::sync::OnceLock<std::collections::HashMap<(&'static str, String), (usize, ElementId, Vec<ElementId>)>>,
}

/// A value built once from a model that does not change after analysis. A copy of the model starts without it (the copy may
/// be changed and analyzed again).
pub(crate) struct Memo<T>(std::sync::OnceLock<T>);

impl<T> Memo<T> {
    pub(crate) fn get(&self, build: impl FnOnce() -> T) -> &T {
        self.0.get_or_init(build)
    }
}

impl<T> Default for Memo<T> {
    fn default() -> Self {
        Memo(std::sync::OnceLock::new())
    }
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        Memo::default()
    }
}

impl<T> std::fmt::Debug for Memo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.get().is_some() { "Memo(built)" } else { "Memo(empty)" })
    }
}

impl Model {
    pub fn get(&self, id: ElementId) -> &Element {
        &self.elements[id]
    }

    /// Element by qualified name. Quotes are optional on a segment: `4.0 Risk Analysis::Part`
    /// finds `'4.0 Risk Analysis'::Part`, including names that start with a digit.
    pub fn find(&self, qualified_name: &str) -> Option<&Element> {
        // One table per model: a write plans many lookups on a model with thousands of elements.
        let table = self.by_name.get_or_init(|| {
            let mut t = std::collections::HashMap::with_capacity(self.elements.len());
            for e in &self.elements {
                t.entry(e.qualified_name.clone()).or_insert(e.id);
            }
            t
        });
        if let Some(id) = table.get(qualified_name) {
            return Some(&self.elements[*id]);
        }
        let canon = canonical_name(qualified_name)?;
        if canon == qualified_name {
            return None;
        }
        table.get(&canon).map(|id| &self.elements[*id])
    }

    pub fn children(&self, id: ElementId) -> impl Iterator<Item = &Element> {
        self.elements[id].children.iter().map(move |c| &self.elements[*c])
    }

    pub fn descendants(&self, id: ElementId) -> Vec<ElementId> {
        let mut out = Vec::new();
        let mut stack: Vec<ElementId> = self.elements[id].children.iter().rev().copied().collect();
        while let Some(e) = stack.pop() {
            out.push(e);
            stack.extend(self.elements[e].children.iter().rev().copied());
        }
        out
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    /// One-based line and column of a byte offset.
    pub fn line_col(&self, file: usize, offset: usize) -> (usize, usize) {
        let text = &self.files[file].text;
        // An offset inside a character counts as the start of that character.
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        // One table per model: a reply with thousands of rows asks for thousands of lines of one large file.
        let starts = self.line_starts.get_or_init(|| self.files.iter().map(|f| std::iter::once(0).chain(f.text.match_indices('\n').map(|(i, _)| i + 1)).collect()).collect());
        match starts.get(file) {
            Some(ls) if ls.last().map(|l| *l <= text.len()).unwrap_or(false) => {
                let line = ls.partition_point(|s| *s <= offset);
                (line, offset - ls[line - 1] + 1)
            }
            _ => {
                let before = &text[..offset];
                (before.bytes().filter(|b| *b == b'\n').count() + 1, before.rfind('\n').map(|i| offset - i).unwrap_or(offset + 1))
            }
        }
    }
}

/// The spelling the parser gives a qualified name: every segment quoted when it needs it.
/// `None` for text that is not a name (an unbalanced quote, or an empty segment).
pub fn canonical_name(name: &str) -> Option<String> {
    let mut segs: Vec<String> = Vec::new();
    let (mut cur, mut in_q, mut chars) = (String::new(), false, name.trim().chars().peekable());
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_q => {
                cur.push(c);
                cur.extend(chars.next());
            }
            '\'' => {
                in_q = !in_q;
                cur.push(c);
            }
            ':' if !in_q && chars.peek() == Some(&':') => {
                chars.next();
                segs.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if in_q {
        return None;
    }
    segs.push(cur);
    let mut out = Vec::new();
    for s in segs {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let bare = if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
            // As the lexer reads a quoted name: `\n` is a line break, `\t` a tab, any other escaped character itself.
            crate::lexer::unquote(s)
        } else {
            s.to_string()
        };
        out.push(if bare.starts_with('$') && bare[1..].chars().all(|c| c.is_ascii_digit()) { bare } else { quote_name(&bare) });
    }
    Some(out.join("::"))
}

/// Quote a name segment when it is not a plain identifier.
pub fn quote_name(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.chars().next().unwrap().is_ascii_digit();
    if plain && !RESERVED.contains(&name) {
        name.to_string()
    } else {
        // A line break or a tab in a name is written as the escape the lexer reads back, so a name stays on one line.
        format!("'{}'", name.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n").replace('\t', "\\t"))
    }
}

/// Reserved words of the SysML v2 textual notation.
pub const RESERVED: &[&str] = &[
    "about", "abstract", "accept", "action", "actor", "after", "alias", "all", "allocate", "allocation", "analysis",
    "and", "as", "assert", "assign", "assume", "at", "attribute", "bind", "binding", "by", "calc", "case", "comment",
    "concern", "connect", "connection", "constant", "constraint", "crosses", "decide", "def", "default", "defined",
    "dependency", "derived", "do", "doc", "else", "end", "entry", "enum", "event", "exhibit", "exit", "expose",
    "false", "filter", "first", "flow", "for", "fork", "frame", "from", "hastype", "if", "implies", "import", "in",
    "include", "individual", "inout", "interface", "istype", "item", "join", "language", "library", "locale", "loop",
    "merge", "message", "meta", "metadata", "new", "nonunique", "not", "null", "objective", "occurrence", "of", "or",
    "ordered", "out", "package", "parallel", "part", "perform", "port", "private", "protected", "public",
    "redefines", "ref", "references", "render", "rendering", "rep", "require", "requirement", "return", "satisfy",
    "send", "snapshot", "specializes", "stakeholder", "standard", "state", "subject", "subsets", "succession",
    "terminate", "then", "timeslice", "to", "transition", "true", "until", "use", "variant", "variation",
    "verification", "verify", "via", "view", "viewpoint", "when", "while", "xor",
];
