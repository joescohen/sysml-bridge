use super::{Element, ElementId, ElementKind, Model, Relationship, RelationshipKind};

/// Parse errors (non-fatal — parser is tolerant).
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum ParseError {
    #[error("unexpected end of input at line {line}")]
    UnexpectedEof { line: usize },
}

/// Parse SysML v2 textual notation into a Model.
/// The parser is structural and tolerant: it extracts element kinds, names,
/// types, and nesting but does not validate semantics.
pub fn parse_sysml(input: &str) -> Result<Model, ParseError> {
    let mut parser = Parser::new(input);
    parser.parse_top_level()?;
    Ok(parser.model)
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
    model: Model,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            pos: 0,
            line: 1,
            model: Model::new(),
        }
    }

    fn remaining(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn peek_char(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn advance_char(&mut self) -> Option<char> {
        let ch = self.peek_char()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
        }
        Some(ch)
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.advance_char();
            } else if self.remaining().starts_with("//") {
                // Line comment
                while let Some(c) = self.advance_char() {
                    if c == '\n' {
                        break;
                    }
                }
            } else if self.remaining().starts_with("/*") {
                // Block comment
                self.advance_char();
                self.advance_char();
                let mut depth = 1;
                while depth > 0 {
                    if self.remaining().starts_with("/*") {
                        depth += 1;
                        self.advance_char();
                        self.advance_char();
                    } else if self.remaining().starts_with("*/") {
                        depth -= 1;
                        self.advance_char();
                        self.advance_char();
                    } else if self.advance_char().is_none() {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    /// Skip only literal whitespace characters.
    ///
    /// This is intentionally narrower than `skip_whitespace()`: it must not
    /// consume block comments because `try_parse_doc()` needs to see the
    /// opening `/*` token after the `doc` keyword.
    fn skip_space_chars(&mut self) {
        while let Some(ch) = self.peek_char() {
            if ch.is_whitespace() {
                self.advance_char();
            } else {
                break;
            }
        }
    }

    /// Try to consume a doc comment `/* ... */` and return its text.
    fn try_parse_doc(&mut self) -> Option<String> {
        self.skip_whitespace();
        if !self.remaining().starts_with("doc") {
            return None;
        }
        let saved = (self.pos, self.line);
        self.pos += 3;
        self.skip_space_chars();
        if !self.remaining().starts_with("/*") {
            self.pos = saved.0;
            self.line = saved.1;
            return None;
        }
        self.advance_char(); // /
        self.advance_char(); // *
        let start = self.pos;
        let mut depth = 1;
        while depth > 0 {
            if self.remaining().starts_with("/*") {
                depth += 1;
                self.advance_char();
                self.advance_char();
            } else if self.remaining().starts_with("*/") {
                depth -= 1;
                if depth == 0 {
                    let text = self.input[start..self.pos].trim().to_string();
                    self.advance_char();
                    self.advance_char();
                    return Some(text);
                }
                self.advance_char();
                self.advance_char();
            } else if self.advance_char().is_none() {
                break;
            }
        }
        None
    }

    /// Consume a `comment`'s optional name / `about <refs>` and its `/* ... */`
    /// body (the `comment` keyword has already been read), so the element that
    /// follows the comment is not swallowed.
    fn consume_comment(&mut self) {
        self.skip_space_chars();
        while !self.remaining().starts_with("/*") && !self.at_end() {
            let before = self.pos;
            if self.peek_char() == Some(',') {
                self.advance_char();
            } else if self.peek_word() == "about" {
                self.read_word();
            } else {
                let _ = self.read_dotted_name();
            }
            self.skip_space_chars();
            if self.pos == before {
                break; // no progress (no body present) — stop before the body scan
            }
        }
        if self.remaining().starts_with("/*") {
            self.advance_char(); // /
            self.advance_char(); // *
            let mut depth = 1;
            while depth > 0 {
                if self.remaining().starts_with("/*") {
                    depth += 1;
                    self.advance_char();
                    self.advance_char();
                } else if self.remaining().starts_with("*/") {
                    depth -= 1;
                    self.advance_char();
                    self.advance_char();
                } else if self.advance_char().is_none() {
                    break;
                }
            }
        }
    }

    /// Peek at the next word without consuming it.
    fn peek_word(&self) -> &'a str {
        let rest = self.remaining().trim_start();
        let offset = self.input.len() - rest.len();
        let _ = offset; // just for clarity
        let end = rest
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        &rest[..end]
    }

    /// Consume a word (identifier or keyword).
    fn read_word(&mut self) -> &'a str {
        self.skip_whitespace();
        let start = self.pos;
        while let Some(ch) = self.peek_char() {
            if ch.is_alphanumeric() || ch == '_' {
                self.advance_char();
            } else {
                break;
            }
        }
        &self.input[start..self.pos]
    }

    /// Read a single SysML v2 identifier: a quoted name (`'...'` or `"..."`, with
    /// `\`-escapes, surrounding quotes stripped) or a bare word. Returns `""` when
    /// neither is present. Quoting is required for names with spaces, dots, or a
    /// leading digit (e.g. `'1. Mission Requirements'`), so this is what lets such
    /// names — and references to them — parse at all.
    fn read_ident(&mut self) -> String {
        self.skip_whitespace();
        match self.peek_char() {
            Some(quote @ ('\'' | '"')) => {
                self.advance_char(); // opening quote
                let mut s = String::new();
                while let Some(c) = self.advance_char() {
                    if c == '\\' {
                        if let Some(esc) = self.advance_char() {
                            s.push(esc);
                        }
                    } else if c == quote {
                        break;
                    } else {
                        s.push(c);
                    }
                }
                s
            }
            _ => self.read_word().to_string(),
        }
    }

    /// Read a name which might be a quoted short-name like `<'1'>` followed by a
    /// name (a bare word or a quoted `'...'` identifier).
    fn read_name(&mut self) -> (Option<String>, Option<String>) {
        self.skip_whitespace();
        let mut short_name = None;
        if self.remaining().starts_with('<') {
            self.advance_char(); // <
            let start = self.pos;
            while let Some(ch) = self.peek_char() {
                if ch == '>' {
                    short_name = Some(self.input[start..self.pos].to_string());
                    self.advance_char();
                    break;
                }
                self.advance_char();
            }
            self.skip_whitespace();
        }
        // Read the name (a quoted `'...'`/`"..."` identifier or a bare word).
        let name_word = self.read_ident();
        let name = (!name_word.is_empty()).then_some(name_word);
        (name, short_name)
    }

    /// Read a type reference after `:` or `:>` or `:>>`.
    fn read_type_ref(&mut self) -> Option<String> {
        self.skip_whitespace();
        if self.remaining().starts_with(":>>") {
            self.pos += 3;
        } else if self.remaining().starts_with(":>") {
            self.pos += 2;
        } else if self.remaining().starts_with(':') {
            self.advance_char();
        } else {
            return None;
        }
        self.skip_whitespace();
        Some(self.read_dotted_name())
    }

    /// Read a dotted name like `ISQ::mass` or `vehicle_b.engine.drivePwrPort`, or
    /// one with quoted segments like `'Mission MoEs'::'cruise speed'` (quotes
    /// stripped, so it matches the definition's stored name).
    fn read_dotted_name(&mut self) -> String {
        self.skip_whitespace();
        let mut out = String::new();
        // Handle ~ for conjugation
        if self.remaining().starts_with('~') {
            self.advance_char();
            self.skip_whitespace();
            out.push('~');
        }
        // `::`/`.`-separated segments; each may be a quoted `'...'` identifier
        // (with spaces/dots) or a bare word.
        loop {
            out.push_str(&self.read_ident());
            if self.remaining().starts_with("::") {
                out.push_str("::");
                self.pos += 2;
            } else if self.peek_char() == Some('.') {
                out.push('.');
                self.advance_char();
            } else {
                break;
            }
        }
        out
    }

    /// Read an optional multiplicity like `[2]`, `[1..*]`, `[4..6]`, `[0..1]`, `[*]`.
    fn read_multiplicity(&mut self) -> Option<String> {
        self.skip_whitespace();
        if !self.remaining().starts_with('[') {
            return None;
        }
        self.advance_char(); // [
        let start = self.pos;
        let mut depth = 1;
        while depth > 0 {
            match self.peek_char() {
                Some('[') => {
                    depth += 1;
                    self.advance_char();
                }
                Some(']') => {
                    depth -= 1;
                    if depth == 0 {
                        let text = self.input[start..self.pos].trim().to_string();
                        self.advance_char();
                        return Some(text);
                    }
                    self.advance_char();
                }
                Some(_) => {
                    self.advance_char();
                }
                None => break,
            }
        }
        None
    }

    /// Skip to the matching closing brace, respecting nesting.
    fn skip_body(&mut self) {
        let mut depth = 1;
        while depth > 0 && !self.at_end() {
            match self.peek_char() {
                Some('{') => {
                    depth += 1;
                    self.advance_char();
                }
                Some('}') => {
                    depth -= 1;
                    self.advance_char();
                }
                Some('/') => {
                    // Handle comments inside bodies
                    if self.remaining().starts_with("//") {
                        while let Some(c) = self.advance_char() {
                            if c == '\n' {
                                break;
                            }
                        }
                    } else if self.remaining().starts_with("/*") {
                        self.advance_char();
                        self.advance_char();
                        let mut cdepth = 1;
                        while cdepth > 0 {
                            if self.remaining().starts_with("/*") {
                                cdepth += 1;
                                self.advance_char();
                                self.advance_char();
                            } else if self.remaining().starts_with("*/") {
                                cdepth -= 1;
                                self.advance_char();
                                self.advance_char();
                            } else if self.advance_char().is_none() {
                                break;
                            }
                        }
                    } else {
                        self.advance_char();
                    }
                }
                Some('"') | Some('\'') => {
                    let Some(quote) = self.advance_char() else {
                        break;
                    };
                    while let Some(c) = self.advance_char() {
                        if c == '\\' {
                            self.advance_char();
                        } else if c == quote {
                            break;
                        }
                    }
                }
                Some(_) => {
                    self.advance_char();
                }
                None => break,
            }
        }
    }

    /// Skip to the next semicolon or brace (opening or closing).
    fn skip_to_semi_or_brace(&mut self) {
        while let Some(ch) = self.peek_char() {
            match ch {
                ';' => {
                    self.advance_char();
                    return;
                }
                '{' | '}' => return,
                '/' if self.remaining().starts_with("//") => {
                    while let Some(c) = self.advance_char() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                '/' if self.remaining().starts_with("/*") => {
                    self.skip_whitespace();
                }
                _ => {
                    self.advance_char();
                }
            }
        }
    }

    /// Parse a `connect ... to ...` relationship.
    fn parse_connect(&mut self, parent: ElementId) {
        self.skip_whitespace();
        // Optional multiplicity before source
        let _mult = self.read_multiplicity();
        let source = self.read_dotted_name();
        self.skip_whitespace();
        // Expect `to`
        if self.peek_word() == "to" {
            self.read_word(); // consume "to"
        }
        self.skip_whitespace();
        let _mult2 = self.read_multiplicity();
        let target = self.read_dotted_name();
        if !source.is_empty() && !target.is_empty() {
            self.model.relationships.push(Relationship {
                kind: RelationshipKind::Connect,
                name: None,
                source_path: source,
                target_path: target,
                type_ref: None,
                owner: parent,
            });
        }
        self.skip_to_semi_or_brace();
        if self.remaining().starts_with('{') || self.input[..self.pos].ends_with('{') {
            // Check if we just passed a brace
            if self.input.as_bytes().get(self.pos.wrapping_sub(1)) == Some(&b'{') {
                self.skip_body();
            } else if self.remaining().starts_with('{') {
                self.advance_char();
                self.skip_body();
            }
        }
    }

    /// Parse a `bind x = y` relationship.
    fn parse_bind(&mut self, parent: ElementId) {
        self.skip_whitespace();
        let source = self.read_dotted_name();
        self.skip_whitespace();
        // Expect `=`
        if self.remaining().starts_with('=') {
            self.advance_char();
        }
        self.skip_whitespace();
        let target = self.read_dotted_name();
        if !source.is_empty() && !target.is_empty() {
            self.model.relationships.push(Relationship {
                kind: RelationshipKind::Bind,
                name: None,
                source_path: source,
                target_path: target,
                type_ref: None,
                owner: parent,
            });
        }
        self.skip_to_semi_or_brace();
        if self.remaining().starts_with('{') {
            self.advance_char();
            self.skip_body();
        }
    }

    /// Parse a `flow [name] [of Type] from x to y` or `flow x to y`.
    fn parse_flow(&mut self, name: Option<String>, parent: ElementId) {
        self.skip_whitespace();
        let mut type_ref = None;

        // Check for `of Type` clause
        if self.peek_word() == "of" {
            self.read_word();
            self.skip_whitespace();
            let t = self.read_dotted_name();
            if !t.is_empty() {
                type_ref = Some(t);
            }
        }

        // Check for `from` keyword or direct source
        let source = if self.peek_word() == "from" {
            self.read_word();
            self.skip_whitespace();
            self.read_dotted_name()
        } else {
            self.read_dotted_name()
        };

        self.skip_whitespace();
        if self.peek_word() == "to" {
            self.read_word();
        }
        self.skip_whitespace();
        let target = self.read_dotted_name();

        if !source.is_empty() && !target.is_empty() {
            self.model.relationships.push(Relationship {
                kind: RelationshipKind::Flow,
                name,
                source_path: source,
                target_path: target,
                type_ref,
                owner: parent,
            });
        }
        self.skip_to_semi_or_brace();
        if self.remaining().starts_with('{') {
            self.advance_char();
            self.skip_body();
        }
    }

    /// Read a `= <value>` right-hand side: a quoted string or a bare token,
    /// stopping at `;`, `{`, or `}`.
    fn read_value_text(&mut self) -> String {
        self.skip_whitespace();
        match self.peek_char() {
            Some(q @ ('"' | '\'')) => {
                self.advance_char();
                let mut s = String::new();
                while let Some(c) = self.peek_char() {
                    self.advance_char();
                    if c == q {
                        break;
                    }
                    s.push(c);
                }
                s
            }
            _ => {
                let mut s = String::new();
                while let Some(c) = self.peek_char() {
                    if matches!(c, ';' | '{' | '}') {
                        break;
                    }
                    s.push(c);
                    self.advance_char();
                }
                s.trim().to_string()
            }
        }
    }

    /// Parse a `first x [if guard] then y` succession into a Succession
    /// relationship. A guard expression (the `if <expr>` clause on a decision
    /// branch) is captured into `name` so the viewer can label the edge.
    fn parse_succession(&mut self, parent: ElementId) {
        self.skip_whitespace();
        let source = self.read_dotted_name();
        self.skip_whitespace();

        // Optional guard: `first src if <expr> then tgt`.
        let mut guard: Option<String> = None;
        if self.peek_word() == "if" {
            self.read_word();
            self.skip_whitespace();
            let mut tokens = Vec::new();
            while !self.remaining().is_empty() && self.peek_word() != "then" {
                let w = self.read_word().to_string();
                if w.is_empty() {
                    break;
                }
                tokens.push(w);
                self.skip_whitespace();
            }
            if !tokens.is_empty() {
                guard = Some(tokens.join(" "));
            }
        }

        if self.peek_word() == "then" {
            self.read_word();
        }
        self.skip_whitespace();
        let target = self.read_dotted_name();
        if !source.is_empty() && !target.is_empty() {
            self.model.relationships.push(Relationship {
                kind: RelationshipKind::Succession,
                name: guard,
                source_path: source,
                target_path: target,
                type_ref: None,
                owner: parent,
            });
        }
        self.skip_to_semi_or_brace();
        if self.remaining().starts_with('{') {
            self.advance_char();
            self.skip_body();
        }
    }

    /// Parse `transition [name] first S1 [accept T] [if g] [do e] then S2` into
    /// a Transition relationship. The trigger/guard/effect are composed into
    /// `name` as a `trigger [guard] / effect` edge label.
    fn parse_transition(&mut self, parent: ElementId) {
        self.skip_whitespace();
        // Optional transition name before `first`.
        if self.peek_word() != "first" {
            let _ = self.read_word();
            self.skip_whitespace();
        }
        if self.peek_word() == "first" {
            self.read_word();
            self.skip_whitespace();
        }
        let source = self.read_dotted_name();
        self.skip_whitespace();

        let mut label_parts: Vec<String> = Vec::new();
        if self.peek_word() == "accept" {
            self.read_word();
            self.skip_whitespace();
            let trig = self.read_dotted_name();
            if !trig.is_empty() {
                label_parts.push(trig);
            }
            self.skip_whitespace();
        }
        if self.peek_word() == "if" {
            self.read_word();
            self.skip_whitespace();
            let mut g = Vec::new();
            while !self.remaining().is_empty() && !matches!(self.peek_word(), "then" | "do") {
                let w = self.read_word().to_string();
                if w.is_empty() {
                    break;
                }
                g.push(w);
                self.skip_whitespace();
            }
            if !g.is_empty() {
                label_parts.push(format!("[{}]", g.join(" ")));
            }
        }
        if self.peek_word() == "do" {
            self.read_word();
            self.skip_whitespace();
            let mut e = Vec::new();
            while !self.remaining().is_empty() && self.peek_word() != "then" {
                let w = self.read_word().to_string();
                if w.is_empty() {
                    break;
                }
                e.push(w);
                self.skip_whitespace();
            }
            if !e.is_empty() {
                label_parts.push(format!("/ {}", e.join(" ")));
            }
        }

        if self.peek_word() == "then" {
            self.read_word();
        }
        self.skip_whitespace();
        let target = self.read_dotted_name();
        if !source.is_empty() && !target.is_empty() {
            let name = if label_parts.is_empty() {
                None
            } else {
                Some(label_parts.join(" "))
            };
            self.model.relationships.push(Relationship {
                kind: RelationshipKind::Transition,
                name,
                source_path: source,
                target_path: target,
                type_ref: None,
                owner: parent,
            });
        }
        self.skip_to_semi_or_brace();
        if self.remaining().starts_with('{') {
            self.advance_char();
            self.skip_body();
        }
    }

    /// Parse an `interface name:Type connect source to target` relationship.
    fn parse_interface_usage(&mut self, parent: ElementId) {
        let (name, _short) = self.read_name();
        let type_ref = self.read_type_ref();
        self.skip_whitespace();

        // Look for `connect`
        if self.peek_word() == "connect" {
            self.read_word();
            self.skip_whitespace();
            let _mult = self.read_multiplicity();
            let source = self.read_dotted_name();
            self.skip_whitespace();
            if self.peek_word() == "to" {
                self.read_word();
            }
            self.skip_whitespace();
            let _mult2 = self.read_multiplicity();
            let target = self.read_dotted_name();
            if !source.is_empty() && !target.is_empty() {
                self.model.relationships.push(Relationship {
                    kind: RelationshipKind::Interface,
                    name: name.clone(),
                    source_path: source,
                    target_path: target,
                    type_ref: type_ref.clone(),
                    owner: parent,
                });
            }
        }

        // Create element for the interface usage
        let elem = Element {
            id: 0,
            kind: ElementKind::InterfaceUsage,
            name,
            short_name: None,
            type_ref,
            specializes: Vec::new(),
            multiplicity: None,
            value: None,
            doc: None,
            parent: Some(parent),
            children: Vec::new(),
            is_conjugated: false,
            is_abstract: false,
            is_variation: false,
            qualifiers: Vec::new(),
        };
        let id = self.model.add_element(elem);
        if let Some(p) = self.model.element_mut(parent) {
            p.children.push(id);
        }

        self.skip_to_semi_or_brace();
        if self.remaining().starts_with('{') {
            self.advance_char();
            self.skip_body();
        }
    }

    /// Parse an `allocate source to target` or `allocation name:Type allocate source to target`.
    fn parse_allocation(&mut self, parent: ElementId, has_name: bool) {
        let name = if has_name {
            let (n, _) = self.read_name();
            let _type_ref = self.read_type_ref();
            self.skip_whitespace();
            if self.peek_word() == "allocate" {
                self.read_word();
            }
            n
        } else {
            None
        };

        self.skip_whitespace();
        let source = self.read_dotted_name();
        self.skip_whitespace();
        if self.peek_word() == "to" {
            self.read_word();
        }
        self.skip_whitespace();
        let target = self.read_dotted_name();

        if !source.is_empty() && !target.is_empty() {
            self.model.relationships.push(Relationship {
                kind: RelationshipKind::Allocate,
                name,
                source_path: source,
                target_path: target,
                type_ref: None,
                owner: parent,
            });
        }

        self.skip_to_semi_or_brace();
        if self.remaining().starts_with('{') {
            self.advance_char();
            self.parse_body(parent);
        }
    }

    /// Parse the top-level elements.
    fn parse_top_level(&mut self) -> Result<(), ParseError> {
        while !self.at_end() {
            self.skip_whitespace();
            if self.at_end() {
                break;
            }
            let before = self.pos;
            if let Some(id) = self.parse_member(None)? {
                self.model.root_ids.push(id);
            }
            // Guard: ensure forward progress
            if self.pos == before {
                self.advance_char();
            }
        }
        Ok(())
    }

    /// Parse members inside a `{ ... }` body.
    fn parse_body(&mut self, parent: ElementId) {
        while !self.at_end() {
            self.skip_whitespace();
            if self.at_end() {
                break;
            }
            if self.remaining().starts_with('}') {
                self.advance_char();
                return;
            }
            let before = self.pos;
            match self.parse_member(Some(parent)) {
                Ok(Some(id)) => {
                    if let Some(p) = self.model.element_mut(parent) {
                        p.children.push(id);
                    }
                }
                Ok(None) => {}
                Err(_) => {
                    // Tolerant: skip to next statement
                    self.skip_to_semi_or_brace();
                    if self.remaining().starts_with('{') {
                        self.advance_char();
                        self.skip_body();
                    }
                }
            }
            // Guard: ensure forward progress to prevent infinite loops
            if self.pos == before {
                self.advance_char();
            }
        }
    }

    /// Parse a single member element. Returns the element id if one was created.
    fn parse_member(&mut self, parent: Option<ElementId>) -> Result<Option<ElementId>, ParseError> {
        self.skip_whitespace();
        if self.at_end() {
            return Ok(None);
        }

        // Handle closing brace
        if self.remaining().starts_with('}') {
            return Ok(None);
        }

        // Handle metadata annotations (@...)
        if self.remaining().starts_with('@') {
            self.advance_char();
            self.skip_to_semi_or_brace();
            if self.remaining().starts_with('{') {
                self.advance_char();
                self.skip_body();
            }
            return Ok(None);
        }

        // Handle redefines shorthand `:>>`
        if self.remaining().starts_with(":>>") {
            self.skip_to_semi_or_brace();
            if self.remaining().starts_with('{') {
                self.advance_char();
                self.skip_body();
            }
            return Ok(None);
        }

        // Collect leading qualifiers
        let mut qualifiers: Vec<String> = Vec::new();
        let mut is_abstract = false;
        let mut is_variation = false;

        loop {
            self.skip_whitespace();
            let word = self.peek_word();
            match word {
                "public" | "private" | "protected" => {
                    self.read_word();
                    qualifiers.push(word.to_string());
                }
                "abstract" => {
                    self.read_word();
                    is_abstract = true;
                }
                "variation" => {
                    self.read_word();
                    is_variation = true;
                }
                "redefines" | "perform" | "exhibit" | "ref" | "in" | "out" | "inout" => {
                    self.read_word();
                    qualifiers.push(word.to_string());
                }
                "#logical" | "#physical" | "#mop" => {
                    // Metadata shorthand
                    self.read_word();
                }
                _ => break,
            }

            // Check for # prefix metadata
            self.skip_whitespace();
            if self.remaining().starts_with('#') && !self.remaining().starts_with("#{") {
                self.advance_char();
                self.read_word();
            }
        }

        self.skip_whitespace();
        if self.at_end() || self.remaining().starts_with('}') {
            return Ok(None);
        }

        let keyword = self.peek_word();
        match keyword {
            "package" => {
                self.read_word();
                self.parse_element(
                    ElementKind::Package,
                    parent,
                    qualifiers,
                    is_abstract,
                    is_variation,
                )
            }
            "part" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::PartDef
                } else {
                    ElementKind::PartUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "port" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::PortDef
                } else {
                    ElementKind::PortUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "attribute" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::AttributeDef
                } else {
                    ElementKind::AttributeUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "item" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::ItemDef
                } else {
                    ElementKind::ItemUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "action" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::ActionDef
                } else {
                    ElementKind::ActionUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "state" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::StateDef
                } else {
                    ElementKind::StateUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "connection" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::ConnectionDef
                } else {
                    ElementKind::ConnectionUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "interface" => {
                self.read_word();
                self.skip_whitespace();
                if self.peek_word() == "def" {
                    self.read_word();
                    self.parse_element(
                        ElementKind::InterfaceDef,
                        parent,
                        qualifiers,
                        is_abstract,
                        is_variation,
                    )
                } else if let Some(pid) = parent {
                    // Interface usage with connect
                    self.parse_interface_usage(pid);
                    Ok(None) // already added to parent
                } else {
                    self.parse_element(
                        ElementKind::InterfaceUsage,
                        parent,
                        qualifiers,
                        is_abstract,
                        is_variation,
                    )
                }
            }
            "allocation" => {
                self.read_word();
                self.skip_whitespace();
                if self.peek_word() == "def" {
                    self.read_word();
                    self.parse_element(
                        ElementKind::AllocationDef,
                        parent,
                        qualifiers,
                        is_abstract,
                        is_variation,
                    )
                } else if let Some(pid) = parent {
                    self.parse_allocation(pid, true);
                    Ok(None)
                } else {
                    self.parse_element(
                        ElementKind::AllocationUsage,
                        parent,
                        qualifiers,
                        is_abstract,
                        is_variation,
                    )
                }
            }
            "allocate" => {
                self.read_word();
                if let Some(pid) = parent {
                    self.parse_allocation(pid, false);
                }
                Ok(None)
            }
            "requirement" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::RequirementDef
                } else {
                    ElementKind::RequirementUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "constraint" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::ConstraintDef
                } else {
                    ElementKind::ConstraintUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "concern" => {
                self.read_word();
                self.skip_whitespace();
                if self.peek_word() == "def" {
                    self.read_word();
                }
                self.parse_element(
                    ElementKind::ConcernDef,
                    parent,
                    qualifiers,
                    is_abstract,
                    is_variation,
                )
            }
            "view" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::ViewDef
                } else {
                    ElementKind::ViewUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "viewpoint" => {
                self.read_word();
                self.skip_whitespace();
                if self.peek_word() == "def" {
                    self.read_word();
                }
                self.parse_element(
                    ElementKind::ViewpointDef,
                    parent,
                    qualifiers,
                    is_abstract,
                    is_variation,
                )
            }
            "rendering" => {
                self.read_word();
                self.skip_whitespace();
                if self.peek_word() == "def" {
                    self.read_word();
                }
                self.parse_element(
                    ElementKind::RenderingDef,
                    parent,
                    qualifiers,
                    is_abstract,
                    is_variation,
                )
            }
            "verification" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::VerificationDef
                } else {
                    ElementKind::VerificationUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "enum" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::EnumDef
                } else {
                    ElementKind::EnumUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "occurrence" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::OccurrenceDef
                } else {
                    ElementKind::OccurrenceUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "individual" => {
                self.read_word();
                self.skip_whitespace();
                let kind = if self.peek_word() == "def" {
                    self.read_word();
                    ElementKind::IndividualDef
                } else {
                    ElementKind::IndividualUsage
                };
                self.parse_element(kind, parent, qualifiers, is_abstract, is_variation)
            }
            "signal" => {
                self.read_word();
                self.skip_whitespace();
                if self.peek_word() == "def" {
                    self.read_word();
                }
                self.parse_element(
                    ElementKind::SignalDef,
                    parent,
                    qualifiers,
                    is_abstract,
                    is_variation,
                )
            }
            "metadata" => {
                self.read_word();
                self.skip_whitespace();
                if self.peek_word() == "def" {
                    self.read_word();
                }
                self.parse_element(
                    ElementKind::MetadataDef,
                    parent,
                    qualifiers,
                    is_abstract,
                    is_variation,
                )
            }
            "connect" => {
                self.read_word();
                if let Some(pid) = parent {
                    self.parse_connect(pid);
                } else {
                    self.skip_to_semi_or_brace();
                    if self.remaining().starts_with('{') {
                        self.advance_char();
                        self.skip_body();
                    }
                }
                Ok(None)
            }
            "bind" => {
                self.read_word();
                if let Some(pid) = parent {
                    self.parse_bind(pid);
                } else {
                    self.skip_to_semi_or_brace();
                }
                Ok(None)
            }
            "flow" => {
                self.read_word();
                self.skip_whitespace();
                // Check if there's a name before `from`/`of`
                let next = self.peek_word();
                if next == "from" || next == "of" || next.is_empty() || next.contains('.') {
                    // Unnamed flow: `flow from x to y` or `flow x to y` or `flow of Type from x to y`
                    if let Some(pid) = parent {
                        self.parse_flow(None, pid);
                    } else {
                        self.skip_to_semi_or_brace();
                    }
                } else {
                    // Named flow: `flow generateToAmplify from x to y`
                    let name = self.read_word().to_string();
                    if let Some(pid) = parent {
                        self.parse_flow(Some(name), pid);
                    } else {
                        self.skip_to_semi_or_brace();
                    }
                }
                Ok(None)
            }
            "import" => {
                self.read_word();
                self.skip_to_semi_or_brace();
                Ok(None)
            }
            "alias" => {
                self.read_word();
                self.skip_to_semi_or_brace();
                Ok(None)
            }
            "dependency" => {
                self.read_word();
                self.skip_whitespace();
                if self.peek_word() == "from" {
                    self.read_word();
                }
                self.skip_whitespace();
                let source = self.read_dotted_name();
                self.skip_whitespace();
                if self.peek_word() == "to" {
                    self.read_word();
                }
                self.skip_whitespace();
                let target = self.read_dotted_name();
                if let Some(pid) = parent
                    && !source.is_empty()
                    && !target.is_empty()
                {
                    self.model.relationships.push(Relationship {
                        kind: RelationshipKind::Dependency,
                        name: None,
                        source_path: source,
                        target_path: target,
                        type_ref: None,
                        owner: pid,
                    });
                }
                self.skip_to_semi_or_brace();
                if self.remaining().starts_with('{') {
                    self.advance_char();
                    self.skip_body();
                }
                Ok(None)
            }
            "transition" => {
                self.read_word();
                if let Some(pid) = parent {
                    self.parse_transition(pid);
                } else {
                    self.skip_to_semi_or_brace();
                }
                Ok(None)
            }
            "decide" => {
                self.read_word();
                self.parse_element(ElementKind::DecisionNode, parent, qualifiers, false, false)
            }
            "fork" => {
                self.read_word();
                self.parse_element(ElementKind::ForkNode, parent, qualifiers, false, false)
            }
            "join" => {
                self.read_word();
                self.parse_element(ElementKind::JoinNode, parent, qualifiers, false, false)
            }
            "merge" => {
                self.read_word();
                self.parse_element(ElementKind::MergeNode, parent, qualifiers, false, false)
            }
            "first" => {
                // `first x then y` succession
                self.read_word();
                if let Some(pid) = parent {
                    self.parse_succession(pid);
                } else {
                    self.skip_to_semi_or_brace();
                }
                Ok(None)
            }
            "message" => {
                self.read_word();
                self.parse_element(ElementKind::MessageUsage, parent, qualifiers, false, false)
            }
            "timeslice" => {
                self.read_word();
                self.parse_element(
                    ElementKind::TimesliceUsage,
                    parent,
                    qualifiers,
                    false,
                    false,
                )
            }
            "snapshot" => {
                self.read_word();
                self.parse_element(ElementKind::SnapshotUsage, parent, qualifiers, false, false)
            }
            "variant" => {
                self.read_word();
                // `variant part ...` etc — re-dispatch to the inner keyword
                self.parse_member(parent)
            }
            "satisfy" => {
                self.read_word();
                self.skip_whitespace();
                let req = self.read_dotted_name();
                self.skip_whitespace();
                if self.peek_word() == "by" {
                    self.read_word();
                }
                self.skip_whitespace();
                let satisfier = self.read_dotted_name();
                if let Some(pid) = parent
                    && !req.is_empty()
                    && !satisfier.is_empty()
                {
                    self.model.relationships.push(Relationship {
                        kind: RelationshipKind::Satisfy,
                        name: None,
                        source_path: satisfier,
                        target_path: req,
                        type_ref: None,
                        owner: pid,
                    });
                }
                self.skip_to_semi_or_brace();
                if self.remaining().starts_with('{') {
                    self.advance_char();
                    self.skip_body();
                }
                Ok(None)
            }
            "verify" => {
                // `verify <req>` inside an `objective` body — source is the
                // owning verification def (objective flattens into it).
                self.read_word();
                self.skip_whitespace();
                let target = self.read_dotted_name();
                if let Some(pid) = parent {
                    let src = self.model.element(pid).and_then(|e| e.name.clone());
                    if let Some(src) = src
                        && !target.is_empty()
                    {
                        self.model.relationships.push(Relationship {
                            kind: RelationshipKind::Verify,
                            name: None,
                            source_path: src,
                            target_path: target,
                            type_ref: None,
                            owner: pid,
                        });
                    }
                }
                self.skip_to_semi_or_brace();
                if self.remaining().starts_with('{') {
                    self.advance_char();
                    self.skip_body();
                }
                Ok(None)
            }
            "objective" => {
                // Parse the objective body so `verify` members are captured,
                // flattening them under the owning verification def.
                self.read_word();
                self.skip_whitespace();
                while !self.at_end()
                    && !self.remaining().starts_with('{')
                    && !self.remaining().starts_with(';')
                {
                    self.advance_char();
                }
                if self.remaining().starts_with('{') {
                    self.advance_char();
                    if let Some(pid) = parent {
                        self.parse_body(pid);
                    } else {
                        self.skip_body();
                    }
                } else if self.remaining().starts_with(';') {
                    self.advance_char();
                }
                Ok(None)
            }
            "entry" | "exit" | "do" => {
                // State behavior (entry/do/exit): capture as a compartment feature
                // `<kw> <behavior>` so the state box shows it.
                let kw = self.read_word().to_string();
                self.skip_whitespace();
                let rest = self.read_dotted_name();
                self.skip_to_semi_or_brace();
                if self.remaining().starts_with('{') {
                    self.advance_char();
                    self.skip_body();
                }
                if parent.is_some() {
                    let label = if rest.is_empty() {
                        kw
                    } else {
                        format!("{kw} {rest}")
                    };
                    let elem = Element {
                        id: 0,
                        kind: ElementKind::ActionUsage,
                        name: Some(label),
                        short_name: None,
                        type_ref: None,
                        specializes: Vec::new(),
                        multiplicity: None,
                        value: None,
                        doc: None,
                        parent,
                        children: Vec::new(),
                        is_conjugated: false,
                        is_abstract: false,
                        is_variation: false,
                        qualifiers: Vec::new(),
                    };
                    // Returning the id lets parse_body link it into the state's children.
                    Ok(Some(self.model.add_element(elem)))
                } else {
                    Ok(None)
                }
            }
            "assert" | "assume" | "expose" | "filter" | "frame" | "render" | "subject"
            | "actor" | "stakeholder" | "return"
            | "then" | "accept" | "send" | "if" => {
                // Statement keywords that we skip
                self.read_word();
                self.skip_to_semi_or_brace();
                if self.remaining().starts_with('{') {
                    self.advance_char();
                    self.skip_body();
                }
                Ok(None)
            }
            "comment" => {
                // Standalone comment `comment [name] [about <refs>] /* ... */`.
                // Consume it (and its body) here; the default arm would otherwise
                // skip to the next `{` and swallow the following element.
                self.read_word();
                self.consume_comment();
                Ok(None)
            }
            "doc" => {
                // Standalone doc comment
                let _doc = self.try_parse_doc();
                Ok(None)
            }
            _ => {
                // Unrecognized: could be a dotted-name expression, assignment, etc.
                // Skip to the next semicolon or brace.
                if keyword.is_empty() {
                    // Might be a special character
                    self.advance_char();
                } else {
                    log::trace!(
                        "skipping unrecognized keyword '{}' at line {}",
                        keyword,
                        self.line
                    );
                    self.read_word();
                }
                self.skip_to_semi_or_brace();
                if self.remaining().starts_with('{') {
                    self.advance_char();
                    self.skip_body();
                }
                Ok(None)
            }
        }
    }

    /// Parse an element after the keyword has been consumed.
    fn parse_element(
        &mut self,
        kind: ElementKind,
        parent: Option<ElementId>,
        qualifiers: Vec<String>,
        is_abstract: bool,
        is_variation: bool,
    ) -> Result<Option<ElementId>, ParseError> {
        self.skip_whitespace();

        // Read name (might have short name)
        let (name, short_name) = self.read_name();

        // Check for conjugation (~) in type ref
        let mut is_conjugated = false;

        // Read type reference and specializations
        self.skip_whitespace();
        let mut type_ref = None;
        let mut specializes = Vec::new();

        // Handle `redefines` qualifier on the name
        if self.peek_word() == "redefines" {
            self.read_word();
            self.skip_whitespace();
            let _redef_name = self.read_ident();
        }

        // Parse type/specialization chain
        loop {
            self.skip_whitespace();
            if self.remaining().starts_with(":>>") {
                self.pos += 3;
                self.skip_whitespace();
                let ref_name = self.read_dotted_name();
                if !ref_name.is_empty() {
                    specializes.push(ref_name);
                }
            } else if self.remaining().starts_with(":>") {
                self.pos += 2;
                self.skip_whitespace();
                let ref_name = self.read_dotted_name();
                if !ref_name.is_empty() {
                    specializes.push(ref_name);
                }
            } else if self.remaining().starts_with(':') && !self.remaining().starts_with("::") {
                self.advance_char();
                self.skip_whitespace();
                if self.remaining().starts_with('~') {
                    is_conjugated = true;
                    self.advance_char();
                    self.skip_whitespace();
                }
                let tref = self.read_dotted_name();
                if !tref.is_empty() {
                    type_ref = Some(tref);
                }
            } else {
                break;
            }
        }

        // Read multiplicity
        let multiplicity = self.read_multiplicity();

        // Skip optional qualifiers like `nonunique`, `parallel`, `ordered`, `default`
        loop {
            self.skip_whitespace();
            let w = self.peek_word();
            match w {
                "nonunique" | "parallel" | "ordered" | "default" | "composite" => {
                    self.read_word();
                }
                _ => break,
            }
        }

        // Capture a `= <value>` clause (attribute / requirement text).
        self.skip_whitespace();
        let mut value = None;
        if self.remaining().starts_with('=') && !self.remaining().starts_with("==") {
            self.advance_char();
            value = Some(self.read_value_text());
        }

        // Try to read doc comment
        let doc = self.try_parse_doc();

        // Skip any remaining tokens until `;` or `{` or `}`
        self.skip_whitespace();
        // Skip assignment expressions and other trailing tokens
        while !self.at_end() {
            let ch = self.peek_char();
            match ch {
                Some('{') | Some('}') | Some(';') => break,
                _ => {
                    // Check if we hit a line that starts a new element
                    let w = self.peek_word();
                    if matches!(
                        w,
                        "package"
                            | "part"
                            | "port"
                            | "attribute"
                            | "item"
                            | "action"
                            | "state"
                            | "connection"
                            | "interface"
                            | "allocation"
                            | "allocate"
                            | "requirement"
                            | "constraint"
                            | "concern"
                            | "view"
                            | "viewpoint"
                            | "rendering"
                            | "verification"
                            | "enum"
                            | "occurrence"
                            | "individual"
                            | "signal"
                            | "metadata"
                            | "connect"
                            | "bind"
                            | "flow"
                            | "import"
                            | "transition"
                            | "message"
                            | "satisfy"
                            | "doc"
                    ) {
                        break;
                    }
                    self.advance_char();
                }
            }
        }

        let elem = Element {
            id: 0,
            kind,
            name,
            short_name,
            type_ref,
            specializes,
            multiplicity,
            value,
            doc,
            parent,
            children: Vec::new(),
            is_conjugated,
            is_abstract,
            is_variation,
            qualifiers,
        };
        let id = self.model.add_element(elem);

        // Check for body
        self.skip_whitespace();
        if self.remaining().starts_with('{') {
            self.advance_char();
            self.parse_body(id);
        } else if self.remaining().starts_with(';') {
            self.advance_char();
        }

        Ok(Some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_package() {
        let input = "package Foo { part def Bar; }";
        let model = parse_sysml(input).unwrap();
        assert_eq!(model.root_ids.len(), 1);
        let pkg = model.element(model.root_ids[0]).unwrap();
        assert_eq!(pkg.kind, ElementKind::Package);
        assert_eq!(pkg.name.as_deref(), Some("Foo"));
        assert_eq!(pkg.children.len(), 1);
        let bar = model.element(pkg.children[0]).unwrap();
        assert_eq!(bar.kind, ElementKind::PartDef);
        assert_eq!(bar.name.as_deref(), Some("Bar"));
    }

    fn root_names(input: &str) -> Vec<String> {
        let model = parse_sysml(input).unwrap();
        model
            .root_ids
            .iter()
            .filter_map(|id| model.element(*id).and_then(|e| e.name.clone()))
            .collect()
    }

    #[test]
    fn parse_quoted_names() {
        // Names needing single-quote escaping (leading digit, spaces, dots) parse
        // and keep their unquoted form.
        let input = "package '1. Mission Requirements' { part def 'Air Vehicle'; }";
        let model = parse_sysml(input).unwrap();
        assert_eq!(model.root_ids.len(), 1);
        let pkg = model.element(model.root_ids[0]).unwrap();
        assert_eq!(pkg.kind, ElementKind::Package);
        assert_eq!(pkg.name.as_deref(), Some("1. Mission Requirements"));
        let av = model.element(pkg.children[0]).unwrap();
        assert_eq!(av.kind, ElementKind::PartDef);
        assert_eq!(av.name.as_deref(), Some("Air Vehicle"));
    }

    #[test]
    fn parse_quoted_qualified_type_ref() {
        // A usage typed by a quoted qualified name resolves to the unquoted ref, so
        // it matches the definition's stored name.
        let input = "package P { part v : 'Root'::'Air Vehicle'; }";
        let model = parse_sysml(input).unwrap();
        let p = model.element(model.root_ids[0]).unwrap();
        let v = model.element(p.children[0]).unwrap();
        assert_eq!(v.type_ref.as_deref(), Some("Root::Air Vehicle"));
    }

    #[test]
    fn parse_quoted_top_level_packages_all_appear() {
        // The Skyzer regression: top-level packages named with a leading digit must
        // all appear as named roots (previously dropped, leaving only bare-named
        // packages like `Glossary` visible in the overview).
        let names = root_names(
            "package '0. Mission Statement' { part def A; }\n\
             package '1. Mission Requirements' { part def B; }\n\
             package '2. Mission Structure' { part def 'Air Vehicle'; }\n\
             package Glossary { part def Term; }\n",
        );
        for expected in [
            "0. Mission Statement",
            "1. Mission Requirements",
            "2. Mission Structure",
            "Glossary",
        ] {
            assert!(
                names.contains(&expected.to_string()),
                "missing {expected:?} in {names:?}"
            );
        }
    }

    #[test]
    fn comment_does_not_swallow_following_element() {
        // A `comment /* ... */` (generated models emit these liberally, top-level
        // and in bodies) must not consume the element that follows it.
        assert_eq!(
            root_names("comment /* a note */\npackage A {}\npackage B {}"),
            vec!["A".to_string(), "B".to_string()]
        );
    }

    #[test]
    fn comment_about_targets_then_element() {
        // The `about <refs>` form (including quoted refs) is also consumed cleanly.
        assert_eq!(
            root_names("comment about 'Air Vehicle' /* note */ package P {}"),
            vec!["P".to_string()]
        );
    }

    #[test]
    fn parse_port_with_conjugation() {
        let input = "part def E { port p : ~FuelPort; }";
        let model = parse_sysml(input).unwrap();
        let e = model.element(model.root_ids[0]).unwrap();
        let port = model.element(e.children[0]).unwrap();
        assert!(port.is_conjugated);
        assert_eq!(port.type_ref.as_deref(), Some("FuelPort"));
    }

    #[test]
    fn parse_connect_relationship() {
        let input = "part def V { part a; part b; connect a.p to b.q; }";
        let model = parse_sysml(input).unwrap();
        assert_eq!(model.relationships.len(), 1);
        let rel = &model.relationships[0];
        assert_eq!(rel.kind, RelationshipKind::Connect);
        assert_eq!(rel.source_path, "a.p");
        assert_eq!(rel.target_path, "b.q");
    }

    #[test]
    fn parse_bind_relationship() {
        let input = "part def V { port a; port b; bind a=b; }";
        let model = parse_sysml(input).unwrap();
        assert_eq!(model.relationships.len(), 1);
        assert_eq!(model.relationships[0].kind, RelationshipKind::Bind);
    }

    #[test]
    fn parse_doc_then_first_body_member() {
        let input = r#"
            part def Artifact {
                doc /* Documentation for the parent. */
                part provenance : ProvenanceRecord;
                part policy : AccessControlPolicy;
            }
        "#;
        let model = parse_sysml(input).unwrap();
        let idx = model.name_index();
        let artifact_id = *idx.get("Artifact").unwrap().first().unwrap();
        let artifact = model.element(artifact_id).unwrap();
        let child_names: Vec<&str> = artifact
            .children
            .iter()
            .filter_map(|child_id| model.element(*child_id))
            .filter_map(|child| child.name.as_deref())
            .collect();

        assert_eq!(child_names, vec!["provenance", "policy"]);
    }
}
