//! A recogniser for the expression a write tool puts into a statement: a value, a slot value, a guard, the expression after
//! `after` or `when`, and the body of a constraint. It follows the expression rules of the official SysML v2 textual grammar
//! for the forms that need no braces (the write tools refuse braces): conditional, `??`, `implies`, `or`, `xor`, `and`,
//! equality, classification, comparison, range, arithmetic, power, unary, feature chains, indexing and units in brackets,
//! `->` calls, invocations with positional or named arguments, `new`, and the literals.
//!
//! The text is read with the lexer of the parser, so the two agree on where a name, a number or a quoted text ends. A word
//! the language reserves is no name: `action evil : P::Act` is not an expression, and so cannot go in as one and declare an
//! element nobody asked for. The caller limits the length of the text; nesting is limited here.

use crate::lexer::{lex, Tok, TokKind};
use crate::model::{Span, RESERVED};

const MAX_DEPTH: usize = 48;

struct P<'a> {
    src: &'a str,
    toks: Vec<Tok>,
    i: usize,
    depth: usize,
    /// The names read so far, in the order of the text.
    out: Vec<Named>,
    /// Open brackets after a number: what stands there is a unit, not a name of the model.
    unit: usize,
    /// Open brackets after anything else: an index, or a unit after a name. The two read the same.
    index: usize,
}

/// A name or a feature chain in the text of a clause: `x`, `Mode::cruise`, `ctl.level`.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    /// Where each segment stands in the text.
    pub segs: Vec<Span>,
    /// The function of a call (`f(x)`, `xs->size()`, `new T()`) or a name inside brackets. It is a reference when it
    /// resolves; a library function or a unit does not, and that is no fault.
    pub soft: bool,
}

/// What the text of one clause names.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Names {
    pub names: Vec<Named>,
    /// The name a trigger declares for what it accepts: `s` in `s : Sig`. A guard or an effect of the same statement that
    /// starts a chain with it names that payload, not a model element.
    pub local: Option<Span>,
}

type R = Result<(), String>;

impl<'a> P<'a> {
    fn new(src: &'a str) -> Result<Self, String> {
        let (toks, errs) = lex(src);
        if let Some(e) = errs.first() {
            return Err(e.message.clone());
        }
        if toks.iter().any(|t| t.kind == TokKind::Comment) {
            return Err("a comment is not part of an expression".into());
        }
        Ok(P { src, toks, i: 0, depth: 0, out: Vec::new(), unit: 0, index: 0 })
    }
    fn kind(&self) -> TokKind {
        self.toks[self.i].kind
    }
    fn cur(&self) -> &'a str {
        let s = self.toks[self.i].span;
        &self.src[s.start..s.end]
    }
    fn next_text(&self) -> &'a str {
        let s = self.toks[(self.i + 1).min(self.toks.len() - 1)].span;
        &self.src[s.start..s.end]
    }
    fn bump(&mut self) {
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
    }
    fn p(&self, s: &str) -> bool {
        self.kind() == TokKind::Punct && self.cur() == s
    }
    fn kw(&self, s: &str) -> bool {
        self.kind() == TokKind::Ident && self.cur() == s
    }
    fn eat_p(&mut self, s: &str) -> bool {
        let hit = self.p(s);
        if hit {
            self.bump();
        }
        hit
    }
    fn eat_kw(&mut self, s: &str) -> bool {
        let hit = self.kw(s);
        if hit {
            self.bump();
        }
        hit
    }
    fn found(&self) -> String {
        if self.kind() == TokKind::Eof {
            "the end of the text".into()
        } else {
            format!("'{}'", self.cur())
        }
    }
    fn expect_p(&mut self, s: &str) -> R {
        if self.eat_p(s) {
            Ok(())
        } else {
            Err(format!("expected '{s}', found {}", self.found()))
        }
    }
    fn end(&self) -> R {
        if self.kind() == TokKind::Eof {
            Ok(())
        } else {
            Err(format!("{} does not continue the expression", self.found()))
        }
    }
    /// A name: a quoted name, or a word the language does not reserve.
    fn is_name(&self) -> bool {
        self.kind() == TokKind::Name || (self.kind() == TokKind::Ident && !RESERVED.contains(&self.cur()))
    }
    /// Keep a name that was read; the place of it in `out`, or `None` inside the brackets of a unit.
    fn record(&mut self, segs: Vec<Span>, soft: bool) -> Option<usize> {
        if self.unit > 0 || segs.is_empty() {
            return None;
        }
        self.out.push(Named { segs, soft: soft || self.index > 0 });
        Some(self.out.len() - 1)
    }
    fn name_span(&mut self) -> Result<Span, String> {
        if self.is_name() {
            let s = self.toks[self.i].span;
            self.bump();
            return Ok(s);
        }
        if self.kind() == TokKind::Ident {
            return Err(format!("'{}' is a reserved word of SysML v2 and cannot be a name here (quote it if it is a name: '{}')", self.cur(), self.cur()));
        }
        Err(format!("expected a name, found {}", self.found()))
    }
    /// `A::B::c`
    fn qualified(&mut self) -> Result<Vec<Span>, String> {
        let mut segs = vec![self.name_span()?];
        while self.p("::") {
            self.bump();
            segs.push(self.name_span()?);
        }
        Ok(segs)
    }
    /// `A::B::c` in a place where it names an element of the model.
    fn reference(&mut self, soft: bool) -> R {
        let segs = self.qualified()?;
        self.record(segs, soft);
        Ok(())
    }

    fn expr(&mut self) -> R {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!("nested deeper than {MAX_DEPTH}"));
        }
        let r = if self.eat_kw("if") {
            self.coalesce().and_then(|_| self.expect_p("?")).and_then(|_| self.expr()).and_then(|_| if self.eat_kw("else") { self.expr() } else { Err(format!("expected 'else', found {}", self.found())) })
        } else {
            self.coalesce()
        };
        self.depth -= 1;
        r
    }
    /// One level of left-associative binary operators: `next (op next)*`.
    fn level(&mut self, puncts: &[&str], words: &[&str], next: fn(&mut Self) -> R) -> R {
        next(self)?;
        while (self.kind() == TokKind::Punct && puncts.contains(&self.cur())) || (self.kind() == TokKind::Ident && words.contains(&self.cur())) {
            self.bump();
            next(self)?;
        }
        Ok(())
    }
    fn coalesce(&mut self) -> R {
        self.level(&["??"], &[], Self::implies)
    }
    fn implies(&mut self) -> R {
        self.level(&[], &["implies"], Self::or)
    }
    fn or(&mut self) -> R {
        self.level(&["|"], &["or"], Self::xor)
    }
    fn xor(&mut self) -> R {
        self.level(&[], &["xor"], Self::and)
    }
    fn and(&mut self) -> R {
        self.level(&["&"], &["and"], Self::equality)
    }
    fn equality(&mut self) -> R {
        self.level(&["==", "!=", "===", "!=="], &[], Self::classification)
    }
    fn classification(&mut self) -> R {
        if self.kw("hastype") || self.kw("istype") {
            self.bump();
            return self.reference(false);
        }
        self.relational()?;
        if self.kw("hastype") || self.kw("istype") || self.kw("as") || self.kw("meta") {
            self.bump();
            self.reference(false)?;
        }
        Ok(())
    }
    fn relational(&mut self) -> R {
        self.level(&["<", ">", "<=", ">="], &[], Self::range)
    }
    fn range(&mut self) -> R {
        self.additive()?;
        if self.eat_p("..") {
            self.additive()?;
        }
        Ok(())
    }
    fn additive(&mut self) -> R {
        self.level(&["+", "-"], &[], Self::multiplicative)
    }
    fn multiplicative(&mut self) -> R {
        self.level(&["*", "/", "%"], &[], Self::power)
    }
    fn power(&mut self) -> R {
        self.unary()?;
        if self.p("**") || self.p("^") {
            self.bump();
            self.depth += 1;
            if self.depth > MAX_DEPTH {
                return Err(format!("nested deeper than {MAX_DEPTH}"));
            }
            let r = self.power();
            self.depth -= 1;
            return r;
        }
        Ok(())
    }
    fn unary(&mut self) -> R {
        let mut signs = 0;
        while self.p("+") || self.p("-") || self.p("~") || self.kw("not") {
            self.bump();
            signs += 1;
            if signs > MAX_DEPTH {
                return Err(format!("more than {MAX_DEPTH} signs in a row"));
            }
        }
        if self.eat_kw("all") {
            return self.reference(false);
        }
        self.primary()
    }
    fn primary(&mut self) -> R {
        // The chain that `.name` still continues: `a.b.c` is one name of three segments.
        let mut open = self.base()?;
        loop {
            if self.p(".") {
                self.bump();
                if self.eat_kw("metadata") {
                    open = None;
                    continue;
                }
                let s = self.name_span()?;
                if let Some(i) = open {
                    self.out[i].segs.push(s);
                }
            } else if self.p("[") {
                let after_number = self.i > 0 && self.toks[self.i - 1].kind == TokKind::Number;
                self.bump();
                open = None;
                let depth = if after_number { &mut self.unit } else { &mut self.index };
                *depth += 1;
                let r = self.sequence();
                let depth = if after_number { &mut self.unit } else { &mut self.index };
                *depth -= 1;
                r?;
                self.expect_p("]")?;
            } else if self.p("->") {
                self.bump();
                open = None;
                self.reference(true)?;
                if self.p("(") {
                    self.arguments()?;
                } else {
                    // A function given by name: `xs->reduce add`.
                    self.reference(true)?;
                }
            } else {
                return Ok(());
            }
        }
    }
    /// The place in `out` of the name this base is, when a `.name` after it continues that name.
    fn base(&mut self) -> Result<Option<usize>, String> {
        match self.kind() {
            TokKind::Number | TokKind::Str => {
                self.bump();
                Ok(None)
            }
            TokKind::Punct if self.cur() == "(" => {
                self.bump();
                if self.eat_p(")") {
                    return Ok(None);
                }
                self.sequence()?;
                self.expect_p(")").map(|_| None)
            }
            // `.5` is a real number; `*` alone is the unbounded value.
            TokKind::Punct if self.cur() == "." && self.toks[self.i + 1].kind == TokKind::Number && self.toks[self.i + 1].span.start == self.toks[self.i].span.end => {
                self.bump();
                self.bump();
                Ok(None)
            }
            TokKind::Punct if self.cur() == "*" => {
                self.bump();
                Ok(None)
            }
            TokKind::Ident if matches!(self.cur(), "true" | "false" | "null") => {
                self.bump();
                Ok(None)
            }
            TokKind::Ident if self.cur() == "new" => {
                self.bump();
                self.reference(true)?;
                self.arguments().map(|_| None)
            }
            // In a value, `5s` is a number with a unit typed without its brackets far more often than a name.
            TokKind::Ident if self.cur().starts_with(|c: char| c.is_ascii_digit()) => Err(format!(
                "'{0}' is not a value: write a quantity as {1} [{2}], or a name that starts with a digit in quotes ('{0}')",
                self.cur(), self.cur().trim_end_matches(|c: char| !c.is_ascii_digit()), self.cur().trim_start_matches(|c: char| c.is_ascii_digit())
            )),
            TokKind::Ident | TokKind::Name => {
                let segs = self.qualified()?;
                if self.p("(") {
                    self.record(segs, true);
                    self.arguments()?;
                    return Ok(None);
                }
                Ok(self.record(segs, false))
            }
            TokKind::Eof => Err("the expression stops where a value or a name must follow".into()),
            _ => Err(format!("expected a value or a name, found {}", self.found())),
        }
    }
    /// `e, e, e`
    fn sequence(&mut self) -> R {
        self.expr()?;
        while self.eat_p(",") {
            if self.p(")") || self.p("]") {
                break;
            }
            self.expr()?;
        }
        Ok(())
    }
    /// `(a, b)` or `(x = a, y = b)`
    fn arguments(&mut self) -> R {
        self.expect_p("(")?;
        if self.eat_p(")") {
            return Ok(());
        }
        loop {
            if self.is_name() && self.next_text() == "=" {
                self.bump();
                self.bump();
            }
            self.expr()?;
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p(")")
    }
    /// `a.b.c` or `A::b.c`: a feature that exists, by name.
    fn chain(&mut self) -> R {
        let mut segs = self.qualified()?;
        while self.eat_p(".") {
            segs.push(self.name_span()?);
        }
        self.record(segs, false);
        Ok(())
    }
    fn effect(&mut self) -> R {
        if self.eat_kw("send") {
            if !self.kw("via") && !self.kw("to") {
                self.expr()?;
            }
            let mut part = false;
            if self.eat_kw("via") {
                self.expr()?;
                part = true;
            }
            if self.eat_kw("to") {
                self.expr()?;
                part = true;
            }
            if !part && self.i == 1 {
                return Err("'send' needs what to send".into());
            }
        } else if self.eat_kw("assign") {
            self.chain()?;
            self.expect_p(":=")?;
            self.expr()?;
        } else {
            self.chain().map_err(|e| format!("{e}; an effect is the name of an action, 'send X to Y' or 'assign a := b'"))?;
        }
        self.end()
    }
    /// What stands after `accept`: `after <expression>`, `at <expression>`, `when <expression>`, or what is accepted
    /// (`Sig`, `s : Sig`), each with an optional `via <port>`. Returns the name the trigger declares.
    fn trigger(&mut self) -> Result<Option<Span>, String> {
        let mut local = None;
        if self.kw("after") || self.kw("at") || self.kw("when") {
            self.bump();
            self.expr()?;
        } else {
            if self.is_name() && self.next_text() == ":" {
                local = Some(self.name_span()?);
                self.bump();
            }
            self.reference(false)?;
        }
        if self.eat_kw("via") {
            self.expr()?;
        }
        self.end().map(|_| local)
    }
}

/// The names in the text of a clause, or `None` when the text is not of the forms this module reads (a body in braces, a
/// declaration). `kind` is `trigger`, `effect`, or anything else for an expression.
pub fn names(kind: &str, text: &str) -> Option<Names> {
    let mut p = P::new(text).ok()?;
    let local = match kind {
        "trigger" => p.trigger().ok()?,
        "effect" => p.effect().ok().map(|_| None)?,
        _ => p.expr().and_then(|_| p.end()).ok().map(|_| None)?,
    };
    Some(Names { names: p.out, local })
}

/// `Ok` when `text` is what stands after `accept` in a transition: see `P::trigger`.
pub fn trigger(text: &str) -> Result<(), String> {
    P::new(text)?.trigger().map(|_| ())
}

/// `Ok` when `text` is one expression and nothing else.
pub fn expression(text: &str) -> Result<(), String> {
    let mut p = P::new(text)?;
    p.expr()?;
    p.end()
}

/// `Ok` when `text` is the effect of a transition as a write tool takes it: the name of an action (`run`, `ctl.reset`),
/// `send <expression> [via <expression>] [to <expression>]`, or `assign <feature> := <expression>`. A declaration
/// (`action a : A`) is not one: it would make an element inside the transition that no tool lists as created.
pub fn effect(text: &str) -> Result<(), String> {
    P::new(text)?.effect()
}

/// `Ok` when `text` is a name or a qualified name, with nothing after it.
pub fn qualified_name(text: &str) -> Result<(), String> {
    let mut p = P::new(text)?;
    p.qualified()?;
    p.end()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_forms_of_the_grammar_are_read() {
        for ok in [
            "5", "12 [V]", "9.8 [m/s^2]", "mass * 2", "-x", "not ready", "a and b or c xor d", "a implies b", "x ?? 0", "if a ? 1 else 2", "level > 5", "a == b", "a !== b", "1..5",
            "Mode::cruise", "e.m", "'Error Recovered'", "\"5 s\"", "f(1, 2)", "f(x = 1, y = 2)", "xs->size()", "xs->reduce add", "new P::T(1)", "(1, 2, 3)", "()", "x istype T", "x as T", "all T", "2 ** 3 ^ 2", ".5",
            "true", "null", "a[1]", "start", "ISQ::mass", "x.metadata", "- - 1", "1e3 * 2.5E-2",
        ] {
            assert!(expression(ok).is_ok(), "{ok}: {:?}", expression(ok));
        }
    }

    #[test]
    fn text_that_is_no_expression_is_refused() {
        for bad in [
            "action evil : P::Act", "part x", "x : T", "a b", "1 +", "= 1", "a ==", "(1", "1)", "a,", "x := 1", "5s", "1.", "a . . b", "a ? b : c", "f(", "f(x = )", "state", "a then b", "in", "ref x", "x :> y",
            "1 2", "a::", "::a", "[V]", "a[", "if a ? 1", "new", "not", "",
        ] {
            assert!(expression(bad).is_err(), "{bad} was read as an expression");
        }
    }

    #[test]
    fn an_effect_is_a_name_a_send_or_an_assign() {
        for ok in ["run", "ctl.reset", "P::Act", "'Reset All'", "send Sig to ctl", "send Sig() via p", "send x via p to q", "assign count := count + 1", "assign a.b := 0"] {
            assert!(effect(ok).is_ok(), "{ok}: {:?}", effect(ok));
        }
        for bad in ["action evil : P::Act", "action evil", "run stop", "send", "assign x = 1", "assign := 1", "x + 1", "run()", "perform run", "accept S", "run : A", "send Sig to", "entry x"] {
            assert!(effect(bad).is_err(), "{bad} was read as an effect");
        }
    }

    #[test]
    fn deep_nesting_is_refused_and_does_not_overflow() {
        let deep = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
        assert!(expression(&deep).unwrap_err().contains("nested deeper"));
        assert!(expression(&format!("{}1", "-".repeat(5000))).is_err());
        assert!(expression(&format!("1{}", " ** 1".repeat(5000))).is_err());
    }
}
