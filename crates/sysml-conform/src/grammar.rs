//! Turns the vendored grammar files into the tables the recogniser runs on.
//!
//! Rule lookup order: the corrections file, then the SysML file, then the KerML
//! file. Only rules reachable from `RootNamespace` are compiled, so the KerML file
//! supplies just the rules the SysML file refers to and does not define itself
//! (expressions, qualified names and the like).

use crate::kebnf::{self, Expr, RuleDef};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const START_RULE: &str = "RootNamespace";

/// Token classes the lexer produces. Their lexical rules in the KerML file are
/// partly prose, so the lexer implements them by hand (see `lexer.rs`).
pub const TOKEN_CLASSES: [&str; 5] = [
    "NAME",
    "STRING_VALUE",
    "REGULAR_COMMENT",
    "DECIMAL_VALUE",
    "EXPONENTIAL_VALUE",
];

pub const END: u32 = u32::MAX;

pub struct Sources<'a> {
    pub sysml: &'a str,
    pub kerml: &'a str,
    pub corrections: &'a str,
}

#[derive(Debug, Clone)]
pub struct Correction {
    pub rule: String,
    /// True when the rule replaces one from the release files.
    pub replaces: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct GrammarReport {
    pub sysml_rules: Vec<String>,
    pub kerml_rules: Vec<String>,
    pub corrections: Vec<Correction>,
    /// Word terminals a reachable rule uses that are not SysML reserved words.
    /// The lexer reads such a word as a NAME, so the terminal can never match.
    pub unmatchable_words: Vec<String>,
    pub compiled_rules: usize,
    pub compiled_nonterminals: usize,
    /// Rules rewritten to an equal form that the recogniser reads in fewer steps
    /// (see `flatten_operators` and `drop_repeated_chains`). Each line names the
    /// nonterminal and what was done. No rewrite changes the language.
    pub rewrites: Vec<String>,
}

pub struct Grammar {
    /// Display text per terminal id: a literal such as `part`, or a class name.
    pub terminals: Vec<String>,
    pub terminal_is_class: Vec<bool>,
    pub keywords: HashMap<String, u16>,
    /// Symbols, longest first, for longest-match lexing.
    pub symbols: Vec<(Vec<char>, u16)>,
    pub class_ids: [u16; 5],
    pub nt_base: u32,
    pub nt_count: usize,
    /// Symbol after the dot for each dotted-rule position, or `END`.
    pub sym_at: Vec<u32>,
    pub lhs_at: Vec<u16>,
    pub nullable: Vec<bool>,
    /// Per terminal: (rule head, position after the terminal) for every place the
    /// terminal can be the first token of the rule.
    pub first_term: Vec<Vec<(u16, u32)>>,
    /// Per nonterminal: the same, for places it can be the first part of a rule.
    pub left_parent: Vec<Vec<(u16, u32)>>,
    /// Per nonterminal: every nonterminal that can start it, itself included.
    pub left_closure: Vec<Vec<u64>>,
    pub words: usize,
    pub start_item: u32,
    pub accept_item: u32,
    pub report: GrammarReport,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Sym {
    T(String),
    N(String),
}

struct Lowering {
    rules: Vec<(String, Vec<Sym>)>,
    fresh: usize,
}

impl Lowering {
    fn fresh(&mut self, owner: &str) -> String {
        self.fresh += 1;
        format!("{owner}#{}", self.fresh)
    }

    fn alternatives(&mut self, owner: &str, expr: &Expr) -> Vec<Vec<Sym>> {
        match expr {
            Expr::Alt(alts) => alts.iter().map(|alt| self.sequence(owner, alt)).collect(),
            other => vec![self.sequence(owner, other)],
        }
    }

    fn sequence(&mut self, owner: &str, expr: &Expr) -> Vec<Sym> {
        match expr {
            Expr::Term(text) => vec![Sym::T(text.clone())],
            Expr::Ref(name) if TOKEN_CLASSES.contains(&name.as_str()) => vec![Sym::T(name.clone())],
            Expr::Ref(name) => vec![Sym::N(name.clone())],
            Expr::Seq(items) => items
                .iter()
                .flat_map(|item| self.sequence(owner, item))
                .collect(),
            Expr::Alt(_) => {
                let name = self.fresh(owner);
                for alt in self.alternatives(owner, expr) {
                    self.rules.push((name.clone(), alt));
                }
                vec![Sym::N(name)]
            }
            Expr::Opt(inner) => {
                let name = self.fresh(owner);
                self.rules.push((name.clone(), Vec::new()));
                for alt in self.alternatives(owner, inner) {
                    self.rules.push((name.clone(), alt));
                }
                vec![Sym::N(name)]
            }
            // Repetition is made left-recursive: that keeps the recogniser linear.
            Expr::Star(inner) | Expr::Plus(inner) => {
                let name = self.fresh(owner);
                let alts = self.alternatives(owner, inner);
                if matches!(expr, Expr::Star(_)) {
                    self.rules.push((name.clone(), Vec::new()));
                } else {
                    for alt in &alts {
                        self.rules.push((name.clone(), alt.clone()));
                    }
                }
                for alt in alts {
                    let mut rhs = vec![Sym::N(name.clone())];
                    rhs.extend(alt);
                    self.rules.push((name.clone(), rhs));
                }
                vec![Sym::N(name)]
            }
        }
    }
}

fn is_word(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_alphabetic())
}

fn references(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Ref(name) => out.push(name.clone()),
        Expr::Term(_) => {}
        Expr::Seq(items) | Expr::Alt(items) => items.iter().for_each(|e| references(e, out)),
        Expr::Opt(inner) | Expr::Star(inner) | Expr::Plus(inner) => references(inner, out),
    }
}

/// Removes nonterminals that are only another name for at most one symbol.
/// The language stays the same; the recogniser takes fewer steps per token.
fn inline_renames(rules: &mut Vec<(String, Vec<Sym>)>) {
    loop {
        let mut count: HashMap<&str, usize> = HashMap::new();
        for (lhs, _) in rules.iter() {
            *count.entry(lhs.as_str()).or_default() += 1;
        }
        let candidate = rules.iter().position(|(lhs, rhs)| {
            lhs != START_RULE
                && count[lhs.as_str()] == 1
                && rhs.len() <= 1
                && rhs.first() != Some(&Sym::N(lhs.clone()))
        });
        let Some(index) = candidate else { return };
        let (name, replacement) = rules.remove(index);
        let target = Sym::N(name);
        for (_, rhs) in rules.iter_mut() {
            if rhs.contains(&target) {
                *rhs = rhs
                    .iter()
                    .flat_map(|sym| {
                        if *sym == target {
                            replacement.clone()
                        } else {
                            vec![sym.clone()]
                        }
                    })
                    .collect();
            }
        }
    }
}

/// The rules of `name`, as indexes into `rules`.
fn rules_of(rules: &[(String, Vec<Sym>)], name: &str) -> Vec<usize> {
    rules
        .iter()
        .enumerate()
        .filter(|(_, (lhs, _))| lhs == name)
        .map(|(i, _)| i)
        .collect()
}

/// Rewrites a nonterminal whose rules hold an operator of the form `A = A op A`.
///
/// The official expression rule is `OwnedExpression = OwnedExpression operator
/// OwnedExpression | 'not' OwnedExpression | ...`: flat, with precedence given in
/// a table. For a recogniser every grouping of `1 + 1 + 1 + ...` is one more
/// derivation, and the count of them grows with the cube of the length.
///
/// For any nonterminal `A` whose rules are
///
/// ```text
/// A = A b A        (binary; b is any sequence, one rule per operator)
///   | g A          (prefix; g does not start with A)
///   | A d          (postfix; d does not end with A)
///   | x            (every other rule)
/// ```
///
/// the same language is given by
///
/// ```text
/// A  = A b A1 | A1
/// A1 = g A1 | A2
/// A2 = A2 d | x
/// ```
///
/// Proof. Each new rule is an old rule with `A1` or `A2` (both parts of `A`) in
/// the place of `A`, so the new grammar derives nothing new. The other way, by
/// induction over an old derivation tree: an `x` root is `A2`. A `g A` root: the
/// child is `t1 b t2 ... b tn` with each `t` an `A1`, and `g t1` is an `A1`. An
/// `A d` root: the last `tn` is `g..g h` with `h` an `A2`, `h d` is an `A2`, so
/// `g..g h d` is an `A1`. An `A b A` root: both children are such lists, and the
/// two lists with `b` between them are one list. Nothing is assumed about `b`,
/// `g`, `d` or `x`; they may hold `A` inside (`if A ? A else A`, `( A )`).
///
/// Before the rules are sorted into the four kinds, a rule `A = B` is replaced
/// by the rules of `B` when one of them starts or ends with `A`, and an optional
/// `A` at the start or the end of a rule is written as two rules. Both steps
/// keep the language.
fn flatten_operators(rules: &mut Vec<(String, Vec<Sym>)>, notes: &mut Vec<String>) {
    let mut heads: Vec<String> = Vec::new();
    for (lhs, _) in rules.iter() {
        if !heads.contains(lhs) {
            heads.push(lhs.clone());
        }
    }
    for a in heads {
        let me = Sym::N(a.clone());
        // `X` stands for "`A` or nothing" when its rules are the empty rule and `X = A`.
        let optional = |rules: &[(String, Vec<Sym>)], sym: &Sym| -> bool {
            let Sym::N(name) = sym else { return false };
            if *name == a {
                return false;
            }
            let own = rules_of(rules, name);
            !own.is_empty()
                && own.iter().any(|i| rules[*i].1.as_slice() == [me.clone()])
                && own
                    .iter()
                    .all(|i| rules[*i].1.is_empty() || rules[*i].1.as_slice() == [me.clone()])
        };
        let edge = |rules: &[(String, Vec<Sym>)], rhs: &[Sym]| -> bool {
            [rhs.first(), rhs.last()]
                .into_iter()
                .flatten()
                .any(|s| *s == me || optional(rules, s))
        };
        let mut mine: Vec<Vec<Sym>> = rules_of(rules, &a)
            .into_iter()
            .map(|i| rules[i].1.clone())
            .collect();
        for _ in 0..8 {
            let mut next: Vec<Vec<Sym>> = Vec::new();
            let mut changed = false;
            for rhs in &mine {
                if let [Sym::N(b)] = rhs.as_slice() {
                    let theirs = rules_of(rules, b);
                    if *b != a && theirs.iter().any(|i| edge(rules, &rules[*i].1)) {
                        next.extend(theirs.iter().map(|i| rules[*i].1.clone()));
                        changed = true;
                        continue;
                    }
                }
                if rhs.len() >= 2 && rhs.first().is_some_and(|s| optional(rules, s)) {
                    next.push(rhs[1..].to_vec());
                    let mut with = vec![me.clone()];
                    with.extend_from_slice(&rhs[1..]);
                    next.push(with);
                    changed = true;
                } else if rhs.len() >= 2 && rhs.last().is_some_and(|s| optional(rules, s)) {
                    next.push(rhs[..rhs.len() - 1].to_vec());
                    let mut with = rhs[..rhs.len() - 1].to_vec();
                    with.push(me.clone());
                    next.push(with);
                    changed = true;
                } else {
                    next.push(rhs.clone());
                }
            }
            mine = next;
            if !changed {
                break;
            }
        }
        mine.retain(|rhs| rhs.as_slice() != [me.clone()]);
        let binary = |rhs: &Vec<Sym>| rhs.len() >= 2 && rhs.first() == Some(&me) && rhs.last() == Some(&me);
        if !mine.iter().any(binary) {
            continue;
        }
        let (term, operand) = (format!("{a}~term"), format!("{a}~operand"));
        let mut out: Vec<(String, Vec<Sym>)> = vec![(a.clone(), vec![Sym::N(term.clone())]), (term.clone(), vec![Sym::N(operand.clone())])];
        let (mut n_binary, mut n_prefix, mut n_postfix) = (0, 0, 0);
        for mut rhs in mine {
            if binary(&rhs) {
                *rhs.last_mut().unwrap() = Sym::N(term.clone());
                out.push((a.clone(), rhs));
                n_binary += 1;
            } else if rhs.len() >= 2 && rhs.last() == Some(&me) {
                *rhs.last_mut().unwrap() = Sym::N(term.clone());
                out.push((term.clone(), rhs));
                n_prefix += 1;
            } else if rhs.len() >= 2 && rhs.first() == Some(&me) {
                rhs[0] = Sym::N(operand.clone());
                out.push((operand.clone(), rhs));
                n_postfix += 1;
            } else {
                out.push((operand.clone(), rhs));
            }
        }
        rules.retain(|(lhs, _)| *lhs != a);
        rules.extend(out);
        notes.push(format!(
            "{a}: {n_binary} binary, {n_prefix} prefix and {n_postfix} postfix operator rules made left-recursive"
        ));
    }
}

/// Drops `X = P s L` beside `X = P s Q` when `L` is the list `Q (s Q)+` and every
/// `X` is a `P`.
///
/// The official rule is `FeatureChainExpression = PrimaryExpression '.'
/// ( QualifiedName | FeatureChain )` and `FeatureChain = QualifiedName ( '.'
/// QualifiedName )+`. So `a.b.c.d` is read with a chain of every length at every
/// dot, and the work grows with the square of the length. The second alternative
/// derives nothing the first does not: `P s Q` is an `X`, so it is a `P`, so
/// `P s Q s Q` is an `X` again, and so on for any count. The check below is that
/// shape and nothing wider.
fn drop_repeated_chains(rules: &mut Vec<(String, Vec<Sym>)>, notes: &mut Vec<String>) {
    // `name` is the list `Q (s Q)+`: one rule `Q M`, with `M = s Q | M s Q`.
    let list_of = |rules: &[(String, Vec<Sym>)], name: &str| -> Option<(Sym, Sym)> {
        let own = rules_of(rules, name);
        let [one] = own.as_slice() else { return None };
        let [q, Sym::N(m)] = rules[*one].1.as_slice() else { return None };
        let reps = rules_of(rules, m);
        if reps.len() != 2 {
            return None;
        }
        let first = reps.iter().find(|i| rules[**i].1.len() == 2)?;
        let [s, q2] = rules[*first].1.as_slice() else { return None };
        let again = [Sym::N(m.clone()), s.clone(), q.clone()];
        (matches!(s, Sym::T(_)) && q2 == q && reps.iter().any(|i| rules[*i].1.as_slice() == again)).then(|| (s.clone(), q.clone()))
    };
    let mut swaps: Vec<(usize, Sym)> = Vec::new();
    for (index, (x, rhs)) in rules.iter().enumerate() {
        let [Sym::N(p), s, Sym::N(y)] = rhs.as_slice() else { continue };
        // Every `X` is a `P`.
        if !rules.iter().any(|(lhs, r)| lhs == p && r.as_slice() == [Sym::N(x.clone())]) {
            continue;
        }
        // `Y = Q | L`.
        let alts = rules_of(rules, y);
        if alts.len() != 2 {
            continue;
        }
        let found = alts.iter().find_map(|i| match rules[*i].1.as_slice() {
            [Sym::N(l)] => list_of(rules, l).map(|(sep, q)| (sep, q, *i)),
            _ => None,
        });
        let Some((sep, q, list_rule)) = found else { continue };
        let other = alts.iter().find(|i| **i != list_rule).unwrap();
        if sep == *s && rules[*other].1.as_slice() == [q.clone()] {
            swaps.push((index, q));
            notes.push(format!("{x}: the list alternative of {y} after {p} dropped (the single form repeats)"));
        }
    }
    for (index, q) in swaps {
        rules[index].1[2] = q;
    }
}

/// Rewrites a nonterminal whose rules repeat on the right, `R = b R | x`, to the
/// form that repeats on the left: `R = R0 x`, `R0 = | R0 b`.
///
/// The official rules for a list in parentheses (`(1, 2, 3)` is `Expression ','
/// SequenceExpressionList`) and for `if .. else if .. else if ..` repeat on the
/// right, and each element is a complete `R` on its own. The recogniser then
/// finishes one more `R` at every element for every element before it: the work
/// grows with the square of the count. Repeating on the left it is one step each.
///
/// Both forms give the texts `b b ... b x` and nothing else, whatever `b` and `x`
/// are: an old tree with root `b R` has a child that is `b ... b x` by induction,
/// and one more `b` in front is the same form; a new tree unrolls into the old
/// rule applied once for each `b`.
///
/// Before the rules are sorted, the last symbol `Z` of a rule is replaced by the
/// rules of `Z` when one of them ends with `R` (up to three levels down), which
/// keeps the language.
fn repeat_on_the_left(rules: &mut Vec<(String, Vec<Sym>)>, notes: &mut Vec<String>) {
    let mut heads: Vec<String> = Vec::new();
    for (lhs, _) in rules.iter() {
        if !heads.contains(lhs) {
            heads.push(lhs.clone());
        }
    }
    fn leads_to(rules: &[(String, Vec<Sym>)], z: &str, r: &str, depth: usize) -> bool {
        z != r
            && rules.iter().filter(|(lhs, _)| lhs == z).any(|(_, rhs)| match rhs.last() {
                Some(Sym::N(n)) if n == r => true,
                Some(Sym::N(n)) if depth > 0 => leads_to(rules, n, r, depth - 1),
                _ => false,
            })
    }
    for r in heads {
        let me = Sym::N(r.clone());
        let mut mine: Vec<Vec<Sym>> = rules_of(rules, &r).into_iter().map(|i| rules[i].1.clone()).collect();
        for _ in 0..4 {
            let mut next: Vec<Vec<Sym>> = Vec::new();
            let mut changed = false;
            for rhs in &mine {
                match rhs.last() {
                    Some(Sym::N(z)) if leads_to(rules, z, &r, 2) => {
                        for i in rules_of(rules, z) {
                            let mut longer = rhs[..rhs.len() - 1].to_vec();
                            longer.extend(rules[i].1.iter().cloned());
                            next.push(longer);
                        }
                        changed = true;
                    }
                    _ => next.push(rhs.clone()),
                }
            }
            mine = next;
            if !changed || mine.len() > 64 {
                break;
            }
        }
        mine.retain(|rhs| rhs.as_slice() != [me.clone()]);
        let repeats = |rhs: &Vec<Sym>| rhs.len() >= 2 && rhs.last() == Some(&me);
        if mine.len() > 64 || !mine.iter().any(repeats) {
            continue;
        }
        let before = format!("{r}~before");
        let mut out: Vec<(String, Vec<Sym>)> = vec![(before.clone(), Vec::new())];
        let mut count = 0;
        for mut rhs in mine {
            if repeats(&rhs) {
                rhs.pop();
                rhs.insert(0, Sym::N(before.clone()));
                out.push((before.clone(), rhs));
                count += 1;
            } else {
                rhs.insert(0, Sym::N(before.clone()));
                out.push((r.clone(), rhs));
            }
        }
        rules.retain(|(lhs, _)| *lhs != r);
        rules.extend(out);
        notes.push(format!("{r}: {count} rules that repeat on the right made to repeat on the left"));
    }
}

/// Drops `R = R P` when `P` is a list `m+` and every other rule of `R` ends with
/// the same `P`.
///
/// The official rule is `FilterPackage = ImportDeclaration ( '[' Expression ']'
/// )+`, and an ImportDeclaration may be a FilterPackage again. So the filters of
/// `import A::*[f][g][h]` can be shared out over nested packages in every way,
/// and the work grows with the square of their count. `R P` adds no text: an `R`
/// ends with one or more `m`, and one or more `m` after it is still one or more.
///
/// The first symbol `Z` of a rule is replaced by the rules of `Z` when one of
/// them starts with `R` (up to three levels down), which keeps the language.
fn drop_repeated_tails(rules: &mut Vec<(String, Vec<Sym>)>, notes: &mut Vec<String>) {
    let mut heads: Vec<String> = Vec::new();
    for (lhs, _) in rules.iter() {
        if !heads.contains(lhs) {
            heads.push(lhs.clone());
        }
    }
    fn leads_to(rules: &[(String, Vec<Sym>)], z: &str, r: &str, depth: usize) -> bool {
        z != r
            && rules.iter().filter(|(lhs, _)| lhs == z).any(|(_, rhs)| match rhs.first() {
                Some(Sym::N(n)) if n == r => true,
                Some(Sym::N(n)) if depth > 0 => leads_to(rules, n, r, depth - 1),
                _ => false,
            })
    }
    // `name` is the list `m+`: `P = m | P m`.
    let is_list = |rules: &[(String, Vec<Sym>)], name: &str| -> bool {
        let own = rules_of(rules, name);
        if own.len() != 2 {
            return false;
        }
        let Some(one) = own.iter().find(|i| rules[**i].1.first() != Some(&Sym::N(name.to_string()))) else { return false };
        let mut again = vec![Sym::N(name.to_string())];
        again.extend(rules[*one].1.iter().cloned());
        !rules[*one].1.is_empty() && own.iter().any(|i| rules[*i].1 == again)
    };
    for r in heads {
        let me = Sym::N(r.clone());
        let mut mine: Vec<Vec<Sym>> = rules_of(rules, &r).into_iter().map(|i| rules[i].1.clone()).collect();
        // Only a nonterminal whose every rule ends with one list can have the shape.
        let Some(Sym::N(list)) = mine.first().and_then(|rhs| rhs.last()).cloned() else { continue };
        if !is_list(rules, &list) || mine.iter().any(|rhs| rhs.len() < 2 || rhs.last() != Some(&Sym::N(list.clone()))) {
            continue;
        }
        for _ in 0..4 {
            let mut next: Vec<Vec<Sym>> = Vec::new();
            let mut changed = false;
            for rhs in &mine {
                match rhs.first() {
                    Some(Sym::N(z)) if leads_to(rules, z, &r, 2) => {
                        for i in rules_of(rules, z) {
                            let mut longer = rules[i].1.clone();
                            longer.extend(rhs[1..].iter().cloned());
                            next.push(longer);
                        }
                        changed = true;
                    }
                    _ => next.push(rhs.clone()),
                }
            }
            mine = next;
            if !changed || mine.len() > 64 {
                break;
            }
        }
        let again = [me.clone(), Sym::N(list.clone())];
        if mine.len() > 64 || !mine.iter().any(|rhs| rhs.as_slice() == again) {
            continue;
        }
        mine.retain(|rhs| rhs.as_slice() != again);
        rules.retain(|(lhs, _)| *lhs != r);
        rules.extend(mine.into_iter().map(|rhs| (r.clone(), rhs)));
        notes.push(format!("{r}: the rule that puts the list {list} after a {r} dropped (the list repeats already)"));
    }
}

impl Grammar {
    /// The grammar of the vendored files, with the rewrites that keep the
    /// recogniser's work in step with the text.
    pub fn compile(sources: &Sources<'_>) -> Result<Grammar, String> {
        Grammar::compile_with(sources, true)
    }

    /// `rewrite: false` compiles the rules as the files give them. The tests use
    /// it to show that the rewrites change no verdict.
    pub fn compile_with(sources: &Sources<'_>, rewrite: bool) -> Result<Grammar, String> {
        let sysml = kebnf::parse(sources.sysml).map_err(|e| format!("SysML grammar: {e}"))?;
        let kerml = kebnf::parse(sources.kerml).map_err(|e| format!("KerML grammar: {e}"))?;
        let fixes = kebnf::parse(sources.corrections).map_err(|e| format!("corrections: {e}"))?;

        let find = |rules: &[RuleDef], name: &str| rules.iter().position(|r| r.name == name);
        let reserved_words = {
            let at =
                find(&sysml, "RESERVED_KEYWORD").ok_or("SysML grammar has no RESERVED_KEYWORD")?;
            kebnf::terminal_list(&sysml[at])
        };
        let reserved_symbols = {
            let at =
                find(&kerml, "RESERVED_SYMBOL").ok_or("KerML grammar has no RESERVED_SYMBOL")?;
            kebnf::terminal_list(&kerml[at])
        };

        // Walk from the start rule and pick each rule from the first file that has it.
        let mut report = GrammarReport::default();
        let mut lowering = Lowering {
            rules: Vec::new(),
            fresh: 0,
        };
        let mut queue = vec![START_RULE.to_string()];
        let mut done: BTreeSet<String> = BTreeSet::new();
        while let Some(name) = queue.pop() {
            if TOKEN_CLASSES.contains(&name.as_str()) || !done.insert(name.clone()) {
                continue;
            }
            let in_sysml = find(&sysml, &name);
            let in_kerml = find(&kerml, &name);
            let def = if let Some(at) = find(&fixes, &name) {
                report.corrections.push(Correction {
                    rule: name.clone(),
                    replaces: in_sysml.is_some() || in_kerml.is_some(),
                    reason: fixes[at].comment.clone(),
                });
                &fixes[at]
            } else if let Some(at) = in_sysml {
                report.sysml_rules.push(name.clone());
                &sysml[at]
            } else if let Some(at) = in_kerml {
                report.kerml_rules.push(name.clone());
                &kerml[at]
            } else {
                return Err(format!(
                    "rule `{name}` is referred to but defined in no grammar file"
                ));
            };
            for alt in lowering.alternatives(&name, &def.body) {
                lowering.rules.push((name.clone(), alt));
            }
            references(&def.body, &mut queue);
        }
        for fix in &fixes {
            if !done.contains(&fix.name) {
                return Err(format!(
                    "correction `{}` is not reachable from the start rule",
                    fix.name
                ));
            }
        }
        report.sysml_rules.sort();
        report.kerml_rules.sort();
        report.corrections.sort_by(|a, b| a.rule.cmp(&b.rule));

        let mut rules = lowering.rules;
        inline_renames(&mut rules);
        rules.retain(|(lhs, rhs)| rhs.as_slice() != [Sym::N(lhs.clone())]);
        if rewrite {
            drop_repeated_chains(&mut rules, &mut report.rewrites);
            drop_repeated_tails(&mut rules, &mut report.rewrites);
            flatten_operators(&mut rules, &mut report.rewrites);
            repeat_on_the_left(&mut rules, &mut report.rewrites);
            let mut unique = BTreeSet::new();
            rules.retain(|rule| unique.insert(rule.clone()));
            // Rules that nothing reaches any more would only widen the tables.
            let mut reached: BTreeSet<String> = BTreeSet::new();
            let mut todo = vec![START_RULE.to_string()];
            while let Some(name) = todo.pop() {
                if !reached.insert(name.clone()) {
                    continue;
                }
                for (lhs, rhs) in &rules {
                    if *lhs == name {
                        todo.extend(rhs.iter().filter_map(|s| match s {
                            Sym::N(n) => Some(n.clone()),
                            Sym::T(_) => None,
                        }));
                    }
                }
            }
            rules.retain(|(lhs, _)| reached.contains(lhs));
        }
        let mut unique = BTreeSet::new();
        rules.retain(|rule| unique.insert(rule.clone()));

        // Terminal table: reserved words, symbols, token classes, then any word the
        // grammar uses that is not reserved (it gets an id that no token carries).
        let mut terminals: Vec<String> = Vec::new();
        let mut terminal_is_class: Vec<bool> = Vec::new();
        let mut ids: BTreeMap<String, u16> = BTreeMap::new();
        let mut add = |text: &str, class: bool, ids: &mut BTreeMap<String, u16>| -> u16 {
            if let Some(id) = ids.get(text) {
                return *id;
            }
            let id = terminals.len() as u16;
            terminals.push(text.to_string());
            terminal_is_class.push(class);
            ids.insert(text.to_string(), id);
            id
        };
        let mut keywords = HashMap::new();
        for word in &reserved_words {
            keywords.insert(word.clone(), add(word, false, &mut ids));
        }
        let mut symbol_texts: BTreeSet<String> = reserved_symbols.into_iter().collect();
        let mut unmatchable = BTreeSet::new();
        for (_, rhs) in &rules {
            for sym in rhs {
                if let Sym::T(text) = sym {
                    if TOKEN_CLASSES.contains(&text.as_str()) {
                        continue;
                    }
                    if is_word(text) {
                        if !keywords.contains_key(text) {
                            unmatchable.insert(text.clone());
                        }
                    } else {
                        symbol_texts.insert(text.clone());
                    }
                }
            }
        }
        let mut symbols: Vec<(Vec<char>, u16)> = Vec::new();
        for text in &symbol_texts {
            symbols.push((text.chars().collect(), add(text, false, &mut ids)));
        }
        symbols.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        let mut class_ids = [0u16; 5];
        for (slot, class) in TOKEN_CLASSES.iter().enumerate() {
            class_ids[slot] = add(class, true, &mut ids);
        }
        for word in &unmatchable {
            add(word, false, &mut ids);
        }
        report.unmatchable_words = unmatchable.into_iter().collect();

        // Nonterminal numbering, with a start rule `<start> = RootNamespace` on top.
        let top = "<start>".to_string();
        rules.insert(0, (top.clone(), vec![Sym::N(START_RULE.to_string())]));
        let mut nt_ids: HashMap<String, u16> = HashMap::new();
        let mut nt_names: Vec<String> = Vec::new();
        for (lhs, _) in &rules {
            if !nt_ids.contains_key(lhs) {
                nt_ids.insert(lhs.clone(), nt_names.len() as u16);
                nt_names.push(lhs.clone());
            }
        }
        let nt_count = nt_names.len();
        let nt_base = terminals.len() as u32;

        let mut numbered: Vec<(u16, Vec<u32>)> = Vec::with_capacity(rules.len());
        for (lhs, rhs) in &rules {
            let mut out = Vec::with_capacity(rhs.len());
            for sym in rhs {
                out.push(match sym {
                    Sym::T(text) => ids[text] as u32,
                    Sym::N(name) => {
                        let id = nt_ids
                            .get(name)
                            .ok_or_else(|| format!("rule `{name}` has no production left"))?;
                        nt_base + *id as u32
                    }
                });
            }
            numbered.push((nt_ids[lhs], out));
        }

        let mut nullable = vec![false; nt_count];
        loop {
            let mut changed = false;
            for (lhs, rhs) in &numbered {
                if !nullable[*lhs as usize]
                    && rhs
                        .iter()
                        .all(|s| *s >= nt_base && nullable[(*s - nt_base) as usize])
                {
                    nullable[*lhs as usize] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let words = nt_count.div_ceil(64);
        let mut sym_at = Vec::new();
        let mut lhs_at = Vec::new();
        let mut first_term = vec![Vec::new(); terminals.len()];
        let mut left_parent = vec![Vec::new(); nt_count];
        let mut left_closure = vec![vec![0u64; words]; nt_count];
        for (nt, set) in left_closure.iter_mut().enumerate() {
            set[nt / 64] |= 1 << (nt % 64);
        }
        let (mut start_item, mut accept_item) = (0, 0);
        for (index, (lhs, rhs)) in numbered.iter().enumerate() {
            let base = sym_at.len() as u32;
            if index == 0 {
                start_item = base;
                accept_item = base + 1;
            }
            for (k, sym) in rhs.iter().enumerate() {
                sym_at.push(*sym);
                lhs_at.push(*lhs);
                let prefix_empty = rhs[..k]
                    .iter()
                    .all(|s| *s >= nt_base && nullable[(*s - nt_base) as usize]);
                if prefix_empty {
                    let after = base + k as u32 + 1;
                    if *sym < nt_base {
                        first_term[*sym as usize].push((*lhs, after));
                    } else {
                        let nt = (*sym - nt_base) as usize;
                        left_parent[nt].push((*lhs, after));
                        left_closure[*lhs as usize][nt / 64] |= 1 << (nt % 64);
                    }
                }
            }
            sym_at.push(END);
            lhs_at.push(*lhs);
        }
        loop {
            let mut changed = false;
            for a in 0..nt_count {
                for b in 0..nt_count {
                    if a != b && left_closure[a][b / 64] & (1 << (b % 64)) != 0 {
                        for w in 0..words {
                            let merged = left_closure[a][w] | left_closure[b][w];
                            if merged != left_closure[a][w] {
                                left_closure[a][w] = merged;
                                changed = true;
                            }
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }

        report.compiled_rules = numbered.len();
        report.compiled_nonterminals = nt_count;
        Ok(Grammar {
            terminals,
            terminal_is_class,
            keywords,
            symbols,
            class_ids,
            nt_base,
            nt_count,
            sym_at,
            lhs_at,
            nullable,
            first_term,
            left_parent,
            left_closure,
            words,
            start_item,
            accept_item,
            report,
        })
    }

    /// How a terminal is shown in an "allowed here" list.
    pub fn show_terminal(&self, id: u16) -> String {
        let text = &self.terminals[id as usize];
        if self.terminal_is_class[id as usize] {
            text.clone()
        } else {
            format!("'{text}'")
        }
    }
}
