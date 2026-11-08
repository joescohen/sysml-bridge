//! Tolerant recursive-descent parser for the SysML v2 textual notation.
//!
//! Definitions and usages share one shape:
//! `prefixes kind [def] [<short>] [name] feature-parts (';' | '{' members '}')`.
//! Relationship statements (connect, transition, satisfy, flow, ...) get
//! their own small rules. Expressions are kept as raw text.

use crate::lexer::{clean_comment, lex, unquote, Tok, TokKind};
use crate::model::*;
use std::collections::BTreeMap;

const KINDS: &[&str] = &[
    "part", "attribute", "item", "port", "action", "state", "requirement", "constraint", "calc", "interface",
    "connection", "allocation", "enum", "occurrence", "flow", "concern", "view", "viewpoint", "rendering",
    "metadata", "case", "binding", "message", "class", "struct", "datatype", "assoc", "feature", "function",
    "predicate", "classifier", "type", "behavior", "step", "expr", "connector", "interaction", "metaclass",
    "namespace", "package",
];

/// Keywords that start a feature part and so cannot be an element name.
const TAIL_KW: &[&str] = &[
    "specializes", "subsets", "redefines", "references", "defined", "conjugates", "default", "ordered",
    "nonunique", "parallel", "connect", "from", "to", "of", "by", "about", "first", "then", "accept", "bind",
    "allocate", "via", "disjoint", "unions", "intersects", "differences", "chains", "inverse", "featured",
    "typed", "def", "for", "crosses", "send", "terminate", "assign", "if", "while", "loop",
];

/// Position of a prefix word in the grammar's RefPrefix: direction, `derived`, `abstract` or `variation`, `constant`, then `ref`.
fn prefix_rank(w: &str) -> Option<u8> {
    Some(match w {
        "in" | "out" | "inout" => 0,
        "derived" => 1,
        "abstract" | "variation" => 2,
        "constant" | "readonly" => 3,
        "ref" => 4,
        _ => return None,
    })
}

const PREFIXES: &[&str] = &[
    "public", "private", "protected", "abstract", "variation", "readonly", "derived", "constant", "individual",
    "composite", "portion", "member", "var", "end", "ref", "variant", "return", "event", "snapshot",
    "timeslice", "then", "ordered", "nonunique",
];

/// Owners whose bodies may hold bare expressions.
fn allows_expressions(kind: &str) -> bool {
    ["constraint", "calc", "function", "predicate", "expr", "objective", "analysis", "verification", "case"]
        .iter()
        .any(|k| kind.contains(k))
}

const MAX_NEST: usize = 128;
/// How far the parser looks for the `]` of an end multiplicity.
const MAX_MULTIPLICITY_TOKENS: usize = 64;

pub struct Parser<'a> {
    src: &'a str,
    toks: Vec<Tok>,
    i: usize,
    file: usize,
    els: &'a mut Vec<Element>,
    roots: &'a mut Vec<ElementId>,
    diags: &'a mut Vec<Diagnostic>,
    /// How many bodies deep the parser is; bounded so hostile nesting cannot exhaust the stack.
    nest: usize,
}

pub fn parse_file(
    file: usize,
    src: &str,
    els: &mut Vec<Element>,
    roots: &mut Vec<ElementId>,
    diags: &mut Vec<Diagnostic>,
) {
    let (toks, errs) = lex(src);
    for e in errs {
        diags.push(Diagnostic { file, span: e.span, severity: Severity::Error, message: e.message });
    }
    let mut p = Parser { src, toks, i: 0, file, els, roots, diags, nest: 0 };
    p.members(None);
}

impl<'a> Parser<'a> {
    fn tok(&self) -> Tok {
        self.toks[self.i]
    }
    fn kind(&self) -> TokKind {
        self.toks[self.i].kind
    }
    fn cur(&self) -> &'a str {
        let s = self.toks[self.i].span;
        &self.src[s.start..s.end]
    }
    fn peek(&self, n: usize) -> (TokKind, &'a str) {
        let t = self.toks[(self.i + n).min(self.toks.len() - 1)];
        (t.kind, &self.src[t.span.start..t.span.end])
    }
    fn at_kw(&self, s: &str) -> bool {
        self.kind() == TokKind::Ident && self.cur() == s
    }
    fn at_p(&self, s: &str) -> bool {
        self.kind() == TokKind::Punct && self.cur() == s
    }
    fn at_name(&self) -> bool {
        matches!(self.kind(), TokKind::Ident | TokKind::Name)
    }
    fn bump(&mut self) {
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
    }
    fn prev_end(&self) -> usize {
        if self.i == 0 {
            0
        } else {
            self.toks[self.i - 1].span.end
        }
    }
    fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.diags.push(Diagnostic { file: self.file, span, severity: Severity::Error, message: msg.into() });
    }
    fn warn(&mut self, span: Span, msg: impl Into<String>) {
        self.diags.push(Diagnostic { file: self.file, span, severity: Severity::Warning, message: msg.into() });
    }

    fn new_el(&mut self, owner: Option<ElementId>, start: usize) -> ElementId {
        let id = self.els.len();
        self.els.push(Element {
            id,
            file: self.file,
            kind: String::new(),
            is_def: false,
            name: None,
            implied_name: false,
            short_name: None,
            qualified_name: String::new(),
            owner,
            children: Vec::new(),
            span: Span::new(start, start),
            name_span: None,
            body_span: None,
            prefixes: Vec::new(),
            direction: None,
            multiplicity: None,
            refs: Vec::new(),
            value: None,
            value_span: None,
            doc: None,
            props: BTreeMap::new(),
        });
        match owner {
            Some(o) => self.els[o].children.push(id),
            None => self.roots.push(id),
        }
        id
    }

    /// Fix the qualified name. Must run before any child is parsed.
    fn name_done(&mut self, id: ElementId) {
        if !self.els[id].qualified_name.is_empty() {
            return;
        }
        if self.els[id].name.is_none() {
            let implied = self.els[id]
                .refs
                .iter()
                .find(|r| r.role == RefRole::Redefines)
                .and_then(|r| r.segments.last())
                .map(|s| s.name.clone());
            if let Some(n) = implied {
                self.els[id].name = Some(n);
                self.els[id].implied_name = true;
            }
        }
        let owner = self.els[id].owner;
        let seg = match (&self.els[id].name, self.els[id].implied_name) {
            (Some(n), false) => quote_name(n),
            _ => {
                let idx = match owner {
                    // The element is the newest child at this point, so the search from the end takes one step.
                    Some(o) => self.els[o].children.iter().rposition(|c| *c == id).unwrap_or(0),
                    None => self.roots.iter().rposition(|c| *c == id).unwrap_or(0),
                };
                format!("${}", idx + 1)
            }
        };
        self.els[id].qualified_name = match owner {
            Some(o) => format!("{}::{}", self.els[o].qualified_name, seg),
            None => seg,
        };
    }

    fn finish(&mut self, id: ElementId) {
        self.name_done(id);
        let end = self.prev_end().max(self.els[id].span.start);
        self.els[id].span.end = end;
    }

    fn members(&mut self, owner: Option<ElementId>) {
        if self.nest >= MAX_NEST {
            let s = self.tok().span;
            self.err(s, format!("bodies nested deeper than {MAX_NEST} are not read"));
            // Leave the cursor on the brace that closes this body.
            let mut depth = 0usize;
            loop {
                match (self.kind(), self.cur()) {
                    (TokKind::Eof, _) => break,
                    (TokKind::Punct, "{") => depth += 1,
                    (TokKind::Punct, "}") if depth == 0 => break,
                    (TokKind::Punct, "}") => depth -= 1,
                    _ => {}
                }
                self.bump();
            }
            return;
        }
        self.nest += 1;
        self.members_inner(owner);
        self.nest -= 1;
    }

    fn members_inner(&mut self, owner: Option<ElementId>) {
        loop {
            match self.kind() {
                TokKind::Eof => break,
                TokKind::Punct if self.cur() == "}" => {
                    if owner.is_some() {
                        break;
                    }
                    let s = self.tok().span;
                    self.err(s, "unexpected '}'");
                    self.bump();
                    continue;
                }
                _ => {}
            }
            let before = self.i;
            self.member(owner);
            if self.i == before {
                let s = self.tok().span;
                self.err(s, format!("unexpected '{}'", self.cur()));
                self.bump();
            }
        }
    }

    fn member(&mut self, owner: Option<ElementId>) {
        let start = self.tok().span.start;
        if self.kind() == TokKind::Comment {
            let id = self.new_el(owner, start);
            self.els[id].kind = "comment".into();
            self.els[id].doc = Some(clean_comment(self.cur()));
            self.bump();
            self.finish(id);
            return;
        }
        if self.at_p(";") {
            self.bump();
            return;
        }

        let mut pre: Vec<String> = Vec::new();
        let mut dir: Option<String> = None;
        let mut end_mult: Option<String> = None;
        let mut end_name: Option<(String, Span)> = None;
        let mut meta_refs: Vec<Reference> = Vec::new();
        // The prefix words with their places, for the checks against the official grammar.
        let mut words: Vec<(String, Span)> = Vec::new();
        // The place of `ref` in `end ref part x`: the grammar allows `end ref x` and `end part x`, never both words.
        let mut end_ref_kind: Option<Span> = None;
        // Kept as the words go by: a run of N prefix words is read once, not once for each `ref` in it.
        let mut seen_end = false;
        loop {
            if self.at_p("#") {
                self.meta_prefix(&mut pre, &mut meta_refs);
                continue;
            }
            if self.kind() != TokKind::Ident {
                break;
            }
            let c = self.cur();
            words.push((c.to_string(), self.tok().span));
            if matches!(c, "in" | "out" | "inout") {
                dir = Some(c.to_string());
                self.bump();
            } else if PREFIXES.contains(&c) {
                pre.push(c.to_string());
                seen_end |= c == "end";
                self.bump();
                if c == "ref" && seen_end && self.kind() == TokKind::Ident && (KINDS.contains(&self.cur()) || matches!(self.cur(), "use" | "analysis" | "verification")) {
                    end_ref_kind = Some(words[words.len() - 1].1);
                }
                if c == "end" {
                    // `end [name] [multiplicity] kind ...`
                    if self.at_name() && self.peek(1).1 == "[" {
                        // A multiplicity is a few tokens long; the walk stops there so that many open brackets cost no pass each.
                        let mut j = self.i + 1;
                        while j < self.toks.len() - 1 && j < self.i + 1 + MAX_MULTIPLICITY_TOKENS && self.text_at(j) != "]" {
                            j += 1;
                        }
                        let after = self.text_at(j + 1);
                        if KINDS.contains(&after) || after == "ref" || after == "occurrence" {
                            let span = self.tok().span;
                            let name = if self.kind() == TokKind::Name { unquote(self.cur()) } else { self.cur().to_string() };
                            end_name = Some((name, span));
                            self.bump();
                        }
                    }
                    if self.at_p("[") {
                        let s = self.tok().span.start;
                        self.skip_balanced();
                        let raw = &self.src[s..self.prev_end()];
                        end_mult = Some(raw.trim_start_matches('[').trim_end_matches(']').trim().to_string());
                    }
                }
            } else {
                words.pop();
                break;
            }
        }
        if let Some(sp) = end_ref_kind {
            self.err(sp, "`end ref part x` is not SysML v2: an end feature is written `end part x` or `end ref x` (the grammar does not allow `ref` before a usage keyword after `end`). Remove `ref`.");
        }
        self.check_prefix_words(&words);

        let id = self.new_el(owner, start);
        self.els[id].direction = dir.clone();
        self.els[id].prefixes = pre.clone();
        self.els[id].multiplicity = end_mult;
        if let Some((n, _)) = end_name {
            self.els[id].props.insert("end".into(), n);
        }
        let owner_kind = owner.map(|o| self.els[o].kind.clone()).unwrap_or_default();
        let head = if self.kind() == TokKind::Ident { self.cur() } else { "" };

        match head {
            "library" | "standard" => {
                while self.at_kw("library") || self.at_kw("standard") {
                    self.bump();
                }
                self.els[id].prefixes.push("library".into());
                if self.at_kw("package") {
                    self.bump();
                }
                self.els[id].kind = "package".into();
                self.decl(id);
            }
            "import" | "expose" => self.import(id),
            "alias" => {
                self.bump();
                self.els[id].kind = "alias".into();
                self.short_and_name(id);
                if self.at_kw("for") {
                    self.bump();
                    self.push_chain(id, RefRole::Target);
                }
                self.end_stmt(id);
            }
            "doc" | "comment" => {
                self.els[id].kind = head.into();
                self.bump();
                if self.at_name() && !self.at_kw("locale") && !self.at_kw("about") {
                    self.set_name(id);
                }
                if self.at_kw("about") {
                    self.bump();
                    self.refs_list(id, RefRole::Other);
                }
                if self.at_kw("locale") {
                    self.bump();
                    if self.kind() == TokKind::Str {
                        self.bump();
                    }
                }
                if self.kind() == TokKind::Comment {
                    let text = clean_comment(self.cur());
                    if head == "doc" {
                        if let Some(o) = owner {
                            if self.els[o].doc.is_none() {
                                self.els[o].doc = Some(text.clone());
                            }
                        }
                    }
                    self.els[id].doc = Some(text);
                    self.bump();
                } else {
                    let s = self.tok().span;
                    self.err(s, format!("expected /* ... */ after '{head}'"));
                }
                if self.at_p(";") {
                    self.bump();
                }
            }
            "dependency" => {
                self.bump();
                self.els[id].kind = "dependency".into();
                // `dependency <id> name from ...`: the identification is optional, and either part of it.
                self.short_name(id);
                if self.at_name() && self.peek(1).1 == "from" {
                    self.set_name(id);
                }
                if self.at_kw("from") {
                    self.bump();
                }
                self.refs_list(id, RefRole::Source);
                if self.at_kw("to") {
                    self.bump();
                    self.refs_list(id, RefRole::Target);
                }
                self.end_stmt(id);
            }
            "connect" => {
                self.els[id].kind = "connection".into();
                self.tail(id);
                self.end_stmt(id);
            }
            "bind" => {
                self.els[id].kind = "binding".into();
                self.tail(id);
                self.end_stmt(id);
            }
            "allocate" => {
                self.els[id].kind = "allocation".into();
                self.tail(id);
                self.end_stmt(id);
            }
            "first" => {
                self.els[id].kind = "succession".into();
                self.tail(id);
                self.end_stmt(id);
            }
            "succession" => {
                self.bump();
                if self.at_kw("flow") {
                    self.bump();
                    self.els[id].kind = "flow".into();
                    self.els[id].prefixes.push("succession".into());
                } else {
                    self.els[id].kind = "succession".into();
                }
                self.decl(id);
            }
            "transition" => {
                self.bump();
                self.els[id].kind = "transition".into();
                self.short_name(id);
                if self.at_name() && !matches!(self.cur(), "first" | "accept" | "if" | "do" | "then") {
                    self.set_name(id);
                }
                if self.at_kw("first") {
                    self.bump();
                    self.end_ref(id, RefRole::Source);
                }
                self.transition_rest(id, None);
            }
            "accept" => {
                self.accept_payload(id);
                self.bump();
                let at = self.tok().span.start;
                let payload = self.capture_until(&["if", "do", "then"], true);
                let local = self.clause(id, "trigger", at, &payload, None);
                if self.at_kw("if") || self.at_kw("do") || self.at_kw("then") {
                    self.els[id].kind = "transition".into();
                    self.els[id].props.insert("trigger".into(), payload);
                    self.transition_rest(id, local);
                } else {
                    self.els[id].kind = "accept action".into();
                    self.els[id].props.insert("payload".into(), payload);
                    self.end_stmt(id);
                }
            }
            "if" if owner_kind.contains("state") => {
                self.els[id].kind = "transition".into();
                self.transition_rest(id, None);
            }
            "if" | "while" | "loop" | "for" | "else" => {
                self.els[id].kind = format!("{head} action");
                let s = self.tok().span.start;
                self.skip_control();
                let text = self.src[s..self.prev_end()].to_string();
                self.els[id].props.insert("text".into(), text);
            }
            "assign" | "terminate" | "filter" | "send" => {
                self.els[id].kind = if head == "filter" { "filter".into() } else { format!("{head} action") };
                self.bump();
                let text = self.capture_until(&[], true);
                self.els[id].props.insert("text".into(), text);
                self.end_stmt(id);
            }
            "entry" | "do" | "exit" => {
                self.els[id].kind = format!("{head} action");
                self.bump();
                if self.at_p(";") {
                    self.bump();
                } else if self.at_kw("action") {
                    self.bump();
                    self.decl(id);
                } else if matches!(self.cur(), "send" | "assign" | "accept") {
                    let text = self.capture_until(&[], true);
                    self.els[id].props.insert("text".into(), text);
                    self.end_stmt(id);
                } else {
                    self.ref_decl(id);
                }
            }
            "satisfy" | "verify" | "perform" | "exhibit" | "include" | "assert" | "assume" | "require"
            | "frame" | "render" => {
                self.bump();
                let mut head = head;
                if self.at_kw("not") {
                    self.els[id].prefixes.push("not".into());
                    self.bump();
                }
                if head == "assert" && self.at_kw("satisfy") {
                    head = "satisfy";
                    self.bump();
                }
                while self.at_p("#") {
                    let (mut extra, mut found) = (Vec::new(), Vec::new());
                    self.meta_prefix(&mut extra, &mut found);
                    self.els[id].prefixes.extend(extra);
                    self.els[id].refs.extend(found);
                }
                match self.kind_phrase() {
                    Some(k) => {
                        self.els[id].kind = format!("{head} {k}");
                        self.decl(id);
                    }
                    None => {
                        self.els[id].kind = head.into();
                        self.ref_decl(id);
                    }
                }
            }
            "subject" | "actor" | "stakeholder" | "objective" => {
                self.bump();
                self.els[id].kind = head.into();
                self.decl(id);
            }
            "fork" | "join" | "merge" | "decide" => {
                self.bump();
                self.els[id].kind = format!("{head} node");
                self.decl(id);
            }
            _ => {
                if self.at_p("@") {
                    self.bump();
                    self.els[id].kind = "metadata".into();
                    self.push_chain(id, RefRole::Type);
                    self.tail(id);
                    self.end_stmt(id);
                } else if let Some(k) = self.kind_phrase() {
                    self.els[id].kind = k;
                    self.decl(id);
                } else if pre.iter().any(|p| p == "then") && self.at_name() && pre.len() == 1 && dir.is_none() {
                    // `then target;`
                    self.els[id].kind = "succession".into();
                    self.end_ref(id, RefRole::Target);
                    self.end_stmt(id);
                } else if pre.last().map(|p| p == "event").unwrap_or(false) {
                    self.els[id].kind = "event".into();
                    self.ref_decl(id);
                } else if self.at_kw("def") && pre.iter().any(|p| p == "individual") {
                    // `individual def A :> B;` is an occurrence definition.
                    self.els[id].kind = "occurrence".into();
                    self.decl(id);
                } else if dir.is_some() || !pre.is_empty() {
                    let implied = pre
                        .iter()
                        .rev()
                        .find(|p| matches!(p.as_str(), "end" | "ref" | "variant" | "return"))
                        .cloned()
                        .unwrap_or_else(|| if dir.is_some() { "parameter".into() } else { "feature".into() });
                    self.els[id].kind = implied;
                    self.decl(id);
                } else if owner_kind == "enum" && self.at_name() {
                    self.els[id].kind = "enum value".into();
                    self.decl(id);
                } else if matches!(self.cur(), ":>>" | ":>" | "redefines" | "subsets" | "specializes")
                    && self.kind() != TokKind::Str
                {
                    self.els[id].kind = "feature".into();
                    self.decl(id);
                } else if self.at_name()
                    && (matches!(self.peek(1).1, ":" | "=" | ":>>" | ":>" | ":=")
                        || (self.peek(1).1 == ";" && !allows_expressions(&owner_kind)))
                {
                    self.els[id].kind = "feature".into();
                    self.decl(id);
                } else if head == "rep" || head == "language" {
                    self.els[id].kind = "textual representation".into();
                    // `rep Name language "L" /* body */` or the short form `language "L" /* body */`. The language
                    // and the body are kept so that a tool can read the statement back.
                    if head == "rep" {
                        self.bump();
                        if self.at_name() && !self.at_kw("language") {
                            self.set_name(id);
                        }
                    }
                    if self.at_kw("language") {
                        self.bump();
                        if self.kind() == TokKind::Str {
                            let lang = unquote(self.cur());
                            self.els[id].props.insert("language".into(), lang);
                            self.bump();
                        }
                    }
                    while !matches!(self.kind(), TokKind::Comment | TokKind::Eof) && !self.at_p(";") && !self.at_p("}") {
                        self.bump();
                    }
                    if self.kind() == TokKind::Comment {
                        let body = clean_comment(self.cur());
                        self.els[id].props.insert("body".into(), body);
                        self.bump();
                    } else if self.at_p(";") {
                        self.bump();
                    }
                } else {
                    self.els[id].kind = "expression".into();
                    let s = self.tok().span;
                    let text = self.capture_until(&[], false);
                    if text.is_empty() {
                        // No progress is reported by the caller.
                        self.els.pop();
                        match owner {
                            Some(o) => {
                                self.els[o].children.pop();
                            }
                            None => {
                                self.roots.pop();
                            }
                        }
                        return;
                    }
                    if !allows_expressions(&owner_kind) {
                        let end = self.prev_end();
                        self.warn(Span::new(s.start, end), "statement is not recognized as SysML v2");
                    }
                    // The body of a constraint or a calc: `{ total < 100 }`.
                    self.clause(id, "body", s.start, &text, None);
                    self.els[id].value = Some(text);
                    if self.at_p(";") {
                        self.bump();
                    }
                }
            }
        }
        self.mark_assignment(id, pre.is_empty() && dir.is_none());
        self.els[id].refs.extend(meta_refs);
        self.finish(id);
    }

    /// `level = L3;` with no keyword sets a feature the owner's type declares. The name becomes
    /// a reference so a missing feature is reported and a rename or delete of the feature sees it.
    fn mark_assignment(&mut self, id: ElementId, bare: bool) {
        let e = &self.els[id];
        let (Some(name), Some(span)) = (&e.name, e.name_span) else { return };
        if !bare || e.kind != "feature" || e.implied_name || e.value.is_none() || e.owner.is_none() || e.refs.iter().any(|r| !matches!(r.role, RefRole::Expr | RefRole::Call)) {
            return;
        }
        let r = Reference {
            role: RefRole::Assign,
            text: self.src[span.start..span.end].to_string(),
            span,
            segments: vec![Segment { name: name.clone(), span, resolved: None }],
            conjugated: false,
        };
        self.els[id].refs.push(r);
    }

    fn text_at(&self, j: usize) -> &'a str {
        let t = self.toks[j.min(self.toks.len() - 1)];
        &self.src[t.span.start..t.span.end]
    }

    fn meta_prefix(&mut self, pre: &mut Vec<String>, refs: &mut Vec<Reference>) {
        self.bump();
        let s = self.tok().span.start;
        let mut segments = Vec::new();
        while self.at_name() {
            let name = if self.kind() == TokKind::Name { unquote(self.cur()) } else { self.cur().to_string() };
            segments.push(Segment { name, span: self.tok().span, resolved: None });
            self.bump();
            if self.at_p("::") && matches!(self.peek(1).0, TokKind::Ident | TokKind::Name) {
                self.bump();
            } else {
                break;
            }
        }
        let span = Span::new(s, self.prev_end());
        pre.push(format!("#{}", &self.src[s..span.end]));
        if !segments.is_empty() {
            refs.push(Reference { role: RefRole::Meta, text: self.src[s..span.end].to_string(), span, segments, conjugated: false });
        }
    }

    /// Set the value of `id` from the text up to the end of the statement and read its names.
    fn capture_value(&mut self, id: ElementId) {
        let start = self.tok().span.start;
        let v = self.capture_until(&[], true);
        if !v.is_empty() {
            let span = Span::new(start, self.prev_end());
            self.els[id].value_span = Some(span);
            // The expression grammar reads the names when the value is of its forms; a value with a body in braces
            // is read by the looser scan.
            if crate::agent::expr::names("value", &v).is_some() {
                self.clause(id, "value", start, &v, None);
            } else {
                let found = expr_refs(self.src, span);
                self.els[id].refs.extend(found);
            }
        }
        self.els[id].value = Some(v);
    }

    fn kind_phrase(&mut self) -> Option<String> {
        if self.kind() != TokKind::Ident {
            return None;
        }
        let c = self.cur();
        let next = self.peek(1).1;
        if c == "use" && next == "case" {
            self.bump();
            self.bump();
            return Some("use case".into());
        }
        if c == "assoc" && next == "struct" {
            self.bump();
            self.bump();
            return Some("assoc struct".into());
        }
        if c == "analysis" || c == "verification" {
            self.bump();
            if self.at_kw("case") {
                self.bump();
            }
            return Some(c.into());
        }
        if KINDS.contains(&c) {
            self.bump();
            return Some(c.into());
        }
        None
    }

    /// Prefix words the official grammar does not allow: `readonly`, and an order other than
    /// direction, `derived`, `abstract`/`variation`, `constant`, `ref`.
    fn check_prefix_words(&mut self, words: &[(String, Span)]) {
        let mut last: Option<(u8, &str)> = None;
        for (w, sp) in words {
            if w == "readonly" {
                self.err(*sp, "`readonly` is not a SysML v2 keyword. Write `constant`.");
            }
            let Some(r) = prefix_rank(w) else { continue };
            if let Some((lr, lw)) = last.filter(|(lr, _)| *lr > r) {
                let _ = lr;
                self.err(*sp, format!("`{w}` must come before `{lw}`: the prefix words are written in the order direction, `derived`, `abstract` or `variation`, `constant`, `ref`."));
            }
            last = Some((r, w));
        }
    }

    fn set_name(&mut self, id: ElementId) {
        let span = self.tok().span;
        let name = if self.kind() == TokKind::Name { unquote(self.cur()) } else { self.cur().to_string() };
        self.els[id].name = Some(name);
        self.els[id].name_span = Some(span);
        self.bump();
    }

    /// `<short>`, when it is next.
    fn short_name(&mut self, id: ElementId) {
        if self.at_p("<") && matches!(self.peek(1).0, TokKind::Ident | TokKind::Name | TokKind::Number) && self.peek(2).1 == ">"
        {
            self.bump();
            let raw = self.cur();
            self.els[id].short_name = Some(if self.kind() == TokKind::Name { unquote(raw) } else { raw.to_string() });
            self.bump();
            self.bump();
        }
    }

    fn short_and_name(&mut self, id: ElementId) {
        self.short_name(id);
        match self.kind() {
            TokKind::Name => self.set_name(id),
            TokKind::Ident if !TAIL_KW.contains(&self.cur()) => {
                let w = self.cur().to_string();
                if RESERVED.contains(&w.as_str()) {
                    let sp = self.tok().span;
                    self.err(sp, format!("`{w}` is a reserved word of SysML v2 and cannot be a name unquoted. Quote the name: '{w}'."));
                }
                self.set_name(id)
            }
            _ => {}
        }
    }

    fn decl(&mut self, id: ElementId) {
        if self.at_kw("def") {
            self.els[id].is_def = true;
            self.bump();
        }
        if !self.els[id].is_def && matches!(self.els[id].kind.as_str(), "interface" | "connection" | "flow" | "message") && self.at_name() {
            // `interface a.b to c.d;`
            let mut j = self.i;
            loop {
                let t = self.toks[j.min(self.toks.len() - 1)];
                if matches!(t.kind, TokKind::Ident | TokKind::Name) && matches!(self.text_at(j + 1), "." | "::") {
                    j += 2;
                } else {
                    break;
                }
            }
            if j > self.i && self.text_at(j + 1) == "to" {
                self.end_ref(id, RefRole::Source);
                self.bump();
                self.end_ref(id, RefRole::Target);
                self.end_stmt(id);
                return;
            }
        }
        self.short_and_name(id);
        self.tail(id);
        self.end_stmt(id);
    }

    /// A statement whose first name points at an existing element (`satisfy R`).
    fn ref_decl(&mut self, id: ElementId) {
        if let Some(r) = self.chain(RefRole::Target) {
            if let Some(last) = r.segments.last() {
                self.els[id].name = Some(last.name.clone());
                self.els[id].implied_name = true;
            }
            self.els[id].refs.push(r);
        }
        self.tail(id);
        self.end_stmt(id);
    }

    fn chain(&mut self, role: RefRole) -> Option<Reference> {
        let mut conjugated = false;
        if self.at_p("~") {
            conjugated = true;
            self.bump();
        }
        let start = self.tok().span.start;
        if self.at_p("$") {
            self.bump();
            if self.at_p("::") {
                self.bump();
            }
        }
        let mut segments = Vec::new();
        while self.at_name() {
            let name = if self.kind() == TokKind::Name { unquote(self.cur()) } else { self.cur().to_string() };
            segments.push(Segment { name, span: self.tok().span, resolved: None });
            self.bump();
            if (self.at_p("::") || self.at_p(".")) && matches!(self.peek(1).0, TokKind::Ident | TokKind::Name) {
                self.bump();
            } else {
                break;
            }
        }
        if segments.is_empty() {
            let s = self.tok().span;
            self.err(s, format!("expected a name, found '{}'", self.cur()));
            return None;
        }
        let span = Span::new(start, self.prev_end());
        Some(Reference { role, text: self.src[span.start..span.end].to_string(), span, segments, conjugated })
    }

    fn push_chain(&mut self, id: ElementId, role: RefRole) {
        if let Some(r) = self.chain(role) {
            self.els[id].refs.push(r);
        }
    }

    fn refs_list(&mut self, id: ElementId, role: RefRole) {
        loop {
            self.push_chain(id, role);
            if self.at_p(",") {
                self.bump();
            } else {
                break;
            }
        }
    }

    /// A connection or succession end: `[name ::>] chain [multiplicity]`.
    fn end_ref(&mut self, id: ElementId, role: RefRole) {
        if self.at_p("[") {
            self.skip_balanced();
        }
        if self.at_name() && (self.peek(1).1 == "::>" || self.peek(1).1 == "references") {
            self.bump();
            self.bump();
        }
        self.push_chain(id, role);
        if self.at_p("[") {
            self.skip_balanced();
        }
    }

    fn skip_balanced(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.kind() {
                TokKind::Eof => break,
                TokKind::Punct => match self.cur() {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            self.bump();
                            break;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            self.bump();
        }
    }

    /// Raw text up to `;`, a stop keyword, or an unbalanced closer.
    fn capture_until(&mut self, stops: &[&str], stop_brace: bool) -> String {
        let start = self.tok().span.start;
        let mut end = start;
        let mut depth = 0usize;
        loop {
            match self.kind() {
                TokKind::Eof => break,
                TokKind::Punct => match self.cur() {
                    "(" | "[" => depth += 1,
                    ")" | "]" => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    "{" => {
                        // `seq->select { ... }`, `seq.{ ... }` and `seq.?{ ... }` keep their body inside the expression. A brace after
                        // a plain feature chain (`= a.b { ... }`) opens the body of the declaration.
                        let lambda = self.body_expression_at(self.i);
                        if depth == 0 && stop_brace && !lambda {
                            break;
                        }
                        depth += 1;
                    }
                    "}" => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    ";" if depth == 0 => break,
                    _ => {}
                },
                TokKind::Ident if depth == 0 && stops.contains(&self.cur()) => break,
                _ => {}
            }
            end = self.tok().span.end;
            self.bump();
        }
        self.src[start..end].trim().to_string()
    }

    /// Is the `{` at token `at` the body of an expression (`->name {`, `.{`, `.?{`), not the body of a declaration?
    fn body_expression_at(&self, at: usize) -> bool {
        if at == 0 {
            return false;
        }
        if self.text_at(at - 1) == "." || (self.text_at(at - 1) == "?" && at >= 2 && self.text_at(at - 2) == ".") {
            return true;
        }
        // Walk back over the qualified name of the invoked function to the `->` before it.
        let mut j = at;
        let mut steps = 0;
        while j >= 1 && steps < MAX_MULTIPLICITY_TOKENS && matches!(self.toks[j - 1].kind, TokKind::Ident | TokKind::Name) {
            if j >= 2 && self.text_at(j - 2) == "->" {
                return true;
            }
            if j >= 2 && self.text_at(j - 2) == "::" {
                j -= 2;
                steps += 1;
            } else {
                return false;
            }
        }
        false
    }

    fn skip_control(&mut self) {
        loop {
            match self.kind() {
                TokKind::Eof => break,
                TokKind::Punct if self.cur() == ";" => {
                    self.bump();
                    break;
                }
                TokKind::Punct if self.cur() == "}" => break,
                TokKind::Punct if self.cur() == "{" => {
                    self.skip_balanced();
                    if self.at_kw("else") || self.at_kw("until") {
                        continue;
                    }
                    break;
                }
                _ => self.bump(),
            }
        }
    }

    /// The names in the text of a clause become references of `id`, so that they resolve, are renamed with what they
    /// name and count when it is deleted. `text` starts at byte `at`. A chain that starts with `local`, the name the
    /// trigger of the statement declares, names the payload and is left out. Returns the name a trigger declares.
    fn clause(&mut self, id: ElementId, kind: &str, at: usize, text: &str, local: Option<&str>) -> Option<String> {
        let found = crate::agent::expr::names(kind, text)?;
        let src = self.src;
        let name_of = |s: Span| {
            let raw = &src[at + s.start..at + s.end];
            if raw.starts_with('\'') { unquote(raw) } else { raw.to_string() }
        };
        for n in &found.names {
            let segments: Vec<Segment> = n.segs.iter().map(|s| Segment { name: name_of(*s), span: Span::new(at + s.start, at + s.end), resolved: None }).collect();
            if local.is_some() && segments.first().map(|s| s.name.as_str()) == local {
                continue;
            }
            let span = Span::new(segments[0].span.start, segments[segments.len() - 1].span.end);
            let role = if n.soft { RefRole::Call } else { RefRole::Expr };
            self.els[id].refs.push(Reference { role, text: src[span.start..span.end].to_string(), span, segments, conjugated: false });
        }
        found.local.map(name_of)
    }

    fn transition_rest(&mut self, id: ElementId, mut local: Option<String>) {
        if self.at_kw("accept") {
            self.bump();
            let at = self.tok().span.start;
            let t = self.capture_until(&["if", "do", "then"], true);
            local = self.clause(id, "trigger", at, &t, None);
            self.els[id].props.insert("trigger".into(), t);
        }
        if self.at_kw("if") {
            self.bump();
            let at = self.tok().span.start;
            let g = self.capture_until(&["do", "then"], true);
            self.clause(id, "guard", at, &g, local.as_deref());
            self.els[id].props.insert("guard".into(), g);
        }
        if self.at_kw("do") {
            self.bump();
            let at = self.tok().span.start;
            let e = self.capture_until(&["then"], true);
            self.clause(id, "effect", at, &e, local.as_deref());
            self.els[id].props.insert("effect".into(), e);
        }
        if self.at_kw("then") {
            self.bump();
            self.end_ref(id, RefRole::Target);
        }
        self.end_stmt(id);
    }

    fn import(&mut self, id: ElementId) {
        self.els[id].kind = self.cur().to_string();
        self.bump();
        if self.at_kw("all") {
            self.els[id].props.insert("all".into(), "true".into());
            self.bump();
        }
        let start = self.tok().span.start;
        let mut segments = Vec::new();
        let mut wildcard = String::new();
        while self.at_name() {
            let name = if self.kind() == TokKind::Name { unquote(self.cur()) } else { self.cur().to_string() };
            segments.push(Segment { name, span: self.tok().span, resolved: None });
            self.bump();
            if !self.at_p("::") {
                break;
            }
            let (nk, nt) = self.peek(1);
            if nt == "*" || nt == "**" {
                self.bump();
                wildcard = self.cur().to_string();
                self.bump();
                if self.at_p("::") && self.peek(1).1 == "**" {
                    self.bump();
                    self.bump();
                    wildcard = "**".into();
                }
                break;
            } else if matches!(nk, TokKind::Ident | TokKind::Name) {
                self.bump();
            } else {
                break;
            }
        }
        if segments.is_empty() {
            let s = self.tok().span;
            self.err(s, "expected a name after import");
        } else {
            let span = Span::new(start, self.prev_end());
            self.els[id].refs.push(Reference {
                role: RefRole::Import,
                text: self.src[span.start..span.end].to_string(),
                span,
                segments,
                conjugated: false,
            });
        }
        if !wildcard.is_empty() {
            self.els[id].props.insert("wildcard".into(), wildcard);
        }
        if self.at_p("[") {
            self.skip_balanced();
        }
        self.end_stmt(id);
    }

    fn tail(&mut self, id: ElementId) {
        loop {
            if self.kind() == TokKind::Punct {
                match self.cur() {
                    ":" => {
                        self.bump();
                        self.refs_list(id, RefRole::Type);
                    }
                    ":>" => {
                        self.bump();
                        self.refs_list(id, RefRole::Specializes);
                    }
                    ":>>" => {
                        self.bump();
                        self.refs_list(id, RefRole::Redefines);
                    }
                    "::>" => {
                        self.bump();
                        self.refs_list(id, RefRole::References);
                    }
                    "[" => {
                        let s = self.tok().span.start;
                        self.skip_balanced();
                        let raw = &self.src[s..self.prev_end()];
                        let inner = raw.trim_start_matches('[').trim_end_matches(']').trim();
                        self.els[id].multiplicity = Some(inner.to_string());
                    }
                    "=" | ":=" => {
                        self.bump();
                        self.capture_value(id);
                    }
                    "(" => self.skip_balanced(),
                    _ => break,
                }
                continue;
            }
            if self.kind() != TokKind::Ident {
                break;
            }
            match self.cur() {
                "specializes" | "subsets" => {
                    self.bump();
                    self.refs_list(id, RefRole::Specializes);
                }
                "redefines" => {
                    self.bump();
                    self.refs_list(id, RefRole::Redefines);
                }
                "references" => {
                    self.bump();
                    self.refs_list(id, RefRole::References);
                }
                "defined" | "typed" => {
                    self.bump();
                    if self.at_kw("by") {
                        self.bump();
                    }
                    self.refs_list(id, RefRole::Type);
                }
                "conjugates" | "unions" | "intersects" | "differences" | "chains" => {
                    self.bump();
                    self.refs_list(id, RefRole::Other);
                }
                "disjoint" | "inverse" | "featured" => {
                    self.bump();
                    if matches!(self.cur(), "from" | "of" | "by") {
                        self.bump();
                    }
                    self.refs_list(id, RefRole::Other);
                }
                "ordered" | "nonunique" | "parallel" => {
                    let p = self.cur().to_string();
                    self.els[id].prefixes.push(p);
                    self.bump();
                }
                "default" => {
                    self.bump();
                    if self.at_p("=") || self.at_p(":=") {
                        self.bump();
                    }
                    self.capture_value(id);
                }
                "connect" => {
                    self.bump();
                    if self.at_p("(") {
                        self.bump();
                        let mut first = true;
                        while !self.at_p(")") && self.kind() != TokKind::Eof {
                            let before = self.i;
                            self.end_ref(id, if first { RefRole::Source } else { RefRole::Target });
                            first = false;
                            if self.at_p(",") {
                                self.bump();
                            }
                            if self.i == before {
                                break;
                            }
                        }
                        if self.at_p(")") {
                            self.bump();
                        }
                    } else {
                        self.end_ref(id, RefRole::Source);
                        if self.at_kw("to") {
                            self.bump();
                            self.end_ref(id, RefRole::Target);
                        }
                    }
                }
                "bind" => {
                    self.bump();
                    self.end_ref(id, RefRole::Source);
                    if self.at_p("=") {
                        self.bump();
                        self.end_ref(id, RefRole::Target);
                    }
                }
                "allocate" if self.peek(1).1 == "(" => {
                    self.bump();
                    self.skip_balanced();
                }
                "allocate" | "first" | "from" => {
                    self.bump();
                    self.end_ref(id, RefRole::Source);
                }
                "crosses" => {
                    self.bump();
                    self.refs_list(id, RefRole::Other);
                }
                "to" | "then" => {
                    self.bump();
                    self.end_ref(id, RefRole::Target);
                }
                "by" => {
                    self.bump();
                    self.end_ref(id, RefRole::Source);
                }
                "if" if self.els[id].kind == "succession" => {
                    self.bump();
                    let at = self.tok().span.start;
                    let g = self.capture_until(&["then"], true);
                    self.clause(id, "guard", at, &g, None);
                    self.els[id].props.insert("guard".into(), g);
                }
                "of" => {
                    self.bump();
                    if self.at_name() && self.peek(1).1 == ":" {
                        self.bump();
                        self.bump();
                    }
                    self.push_chain(id, RefRole::Type);
                }
                "about" => {
                    self.bump();
                    self.refs_list(id, RefRole::Other);
                }
                _ => break,
            }
        }
    }

    /// At `accept`: the payload `x : Item` of an accept action is its out parameter (an out pin), so a flow can name it
    /// (`flow of Item from wait.x to next.x`). It becomes a member `out item x : Item` of the action; the tokens are read
    /// again by the caller, so the text and its other readers do not change. A transition's trigger keeps no member.
    fn accept_payload(&mut self, id: ElementId) {
        let save = self.i;
        self.bump();
        let named = self.at_name() && self.peek(1) == (TokKind::Punct, ":") && !matches!(self.peek(2).1, "if" | "do" | "then" | "via" | ";");
        if named {
            // `accept x : T if ...` without a body is a transition: it has no out pin.
            let mut j = self.i + 2;
            let mut transition = false;
            while j < self.toks.len() {
                let t = self.toks[j];
                let w = &self.src[t.span.start..t.span.end];
                if t.kind == TokKind::Punct && matches!(w, ";" | "{" | "}") || t.kind == TokKind::Eof {
                    break;
                }
                if t.kind == TokKind::Ident && matches!(w, "if" | "do" | "then") {
                    transition = true;
                    break;
                }
                j += 1;
            }
            if !transition {
                self.name_done(id);
                let start = self.tok().span.start;
                let c = self.new_el(Some(id), start);
                let name = if self.kind() == TokKind::Name { unquote(self.cur()) } else { self.cur().to_string() };
                self.els[c].kind = "item".into();
                self.els[c].direction = Some("out".into());
                self.els[c].name = Some(name);
                self.els[c].name_span = Some(self.tok().span);
                self.els[c].props.insert("accept_payload".into(), "true".into());
                self.bump();
                self.bump();
                if self.at_name() {
                    self.push_chain(c, RefRole::Type);
                }
                self.finish(c);
            }
        }
        self.i = save;
    }

    fn end_stmt(&mut self, id: ElementId) {
        if self.kind() == TokKind::Ident && self.els[id].kind.contains("action") {
            match self.cur() {
                "send" | "accept" | "assign" | "terminate" => {
                    let node = self.cur().to_string();
                    if node == "accept" {
                        self.name_done(id);
                        self.accept_payload(id);
                    }
                    let text = self.capture_until(&[], true);
                    self.els[id].props.insert("node".into(), node);
                    self.els[id].props.insert("text".into(), text);
                }
                "while" | "loop" | "for" | "if" => {
                    let s = self.tok().span.start;
                    self.name_done(id);
                    self.skip_control();
                    let text = self.src[s..self.prev_end()].to_string();
                    self.els[id].props.insert("text".into(), text);
                    return;
                }
                _ => {}
            }
        }
        if self.at_p("{") {
            self.name_done(id);
            let s = self.tok().span.start;
            self.bump();
            self.members(Some(id));
            if self.at_p("}") {
                self.bump();
            } else {
                let sp = self.tok().span;
                self.err(sp, "missing '}'");
            }
            self.els[id].body_span = Some(Span::new(s, self.prev_end()));
        } else if self.at_p(";") {
            self.bump();
        } else {
            let sp = self.tok().span;
            self.err(sp, format!("expected ';' or '{{', found '{}'", self.cur()));
            self.recover();
        }
    }

    fn recover(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.kind() {
                TokKind::Eof => break,
                TokKind::Punct => match self.cur() {
                    "{" => depth += 1,
                    "}" => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                        if depth == 0 {
                            self.bump();
                            break;
                        }
                    }
                    ";" if depth == 0 => {
                        self.bump();
                        break;
                    }
                    _ => {}
                },
                _ => {}
            }
            self.bump();
        }
    }
}

/// Words that can stand where a name could in an expression.
const EXPR_WORDS: &[&str] = &[
    "true", "false", "null", "not", "and", "or", "xor", "implies", "if", "then", "else", "hastype", "istype", "as", "meta",
    "all", "new", "in", "self", "that", "this", "default", "return", "result", "of",
];

/// Names in the value expression at `span`. Only plain chains are returned: a call, a name next to a
/// literal (a unit), anything inside `[...]` and expressions with lambdas or navigation are left
/// out, because they may not be model names.
fn expr_refs(src: &str, span: Span) -> Vec<Reference> {
    let text = &src[span.start..span.end];
    let (toks, _) = lex(text);
    let tx = |t: &Tok| &text[t.span.start..t.span.end];
    if toks.iter().any(|t| t.kind == TokKind::Punct && matches!(tx(t), "{" | "->" | "@" | "$" | "#")) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let (mut i, mut bracket) = (0usize, 0usize);
    while i < toks.len() {
        let t = toks[i];
        match t.kind {
            TokKind::Punct => {
                match tx(&t) {
                    "[" => bracket += 1,
                    "]" => bracket = bracket.saturating_sub(1),
                    _ => {}
                }
                i += 1;
                continue;
            }
            TokKind::Ident | TokKind::Name if bracket == 0 => {}
            _ => {
                i += 1;
                continue;
            }
        }
        let starts = match i.checked_sub(1).map(|p| toks[p]) {
            None => true,
            Some(p) if p.kind == TokKind::Punct => !matches!(tx(&p), ")" | "]" | "}" | "." | "::"),
            Some(p) if p.kind == TokKind::Ident => matches!(tx(&p), "not" | "and" | "or" | "xor" | "implies" | "then" | "else" | "if" | "in"),
            Some(_) => false,
        };
        let mut j = i;
        let mut segs: Vec<Tok> = vec![t];
        while matches!(toks[j + 1].kind, TokKind::Punct) && matches!(tx(&toks[j + 1]), "." | "::") && matches!(toks[j + 2].kind, TokKind::Ident | TokKind::Name) {
            j += 2;
            segs.push(toks[j]);
        }
        let next_open = toks[j + 1].kind == TokKind::Punct && tx(&toks[j + 1]) == "(";
        if next_open {
            segs.pop();
        }
        let word = t.kind == TokKind::Ident && EXPR_WORDS.contains(&tx(&t));
        if starts && !word && !segs.is_empty() {
            let (a, b) = (segs[0].span.start, segs[segs.len() - 1].span.end);
            let segments = segs
                .iter()
                .map(|g| {
                    let raw = tx(g);
                    let name = if g.kind == TokKind::Name { unquote(raw) } else { raw.to_string() };
                    Segment { name, span: Span::new(span.start + g.span.start, span.start + g.span.end), resolved: None }
                })
                .collect();
            let abs = Span::new(span.start + a, span.start + b);
            out.push(Reference { role: RefRole::Expr, text: src[abs.start..abs.end].to_string(), span: abs, segments, conjugated: false });
        }
        i = j + 1;
    }
    out
}
