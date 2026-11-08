//! Units written in brackets after a number (`12 [V]`, `9.8 [m/s^2]`): which of them resolve, and the import a writer adds.
//!
//! The units of the official `sysml.library` live in the package `SI` (volt `V`, kelvin `K`, second `s`, metre `m` ...) and,
//! for US customary units, in `USCustomaryUnits`; the quantity types (`ISQ::VoltageValue`) live in `ISQ`. A unit resolves when
//! a model element carries its name, when it is written with the library package (`[SI::V]`), or when an import that reaches
//! the library package is in scope (`import SI::*;`, `import SI::**;`, `import SI::V;`) and the library declares that name.
//! The names the library declares are in `library_units.txt`, read from the official library: `kWh`, `mV` and `ms` are not
//! among them, and Cameo 2026x reports `[SI::kWh]` as a reference it cannot resolve.

use crate::model::*;
use std::collections::{HashMap, HashSet};

/// The packages of the official library that hold units, in the order the writer prefers them.
pub const UNIT_LIBRARIES: &[&str] = &["SI", "USCustomaryUnits"];
/// The import a writer adds for a unit it cannot resolve.
pub const DEFAULT_IMPORT: &str = "SI::*";

/// The unit names `lib` (`SI` or `USCustomaryUnits`) declares at its top level.
fn library_names(lib: &str) -> &'static HashSet<&'static str> {
    static NAMES: std::sync::OnceLock<HashMap<&'static str, HashSet<&'static str>>> = std::sync::OnceLock::new();
    static EMPTY: std::sync::OnceLock<HashSet<&'static str>> = std::sync::OnceLock::new();
    let all = NAMES.get_or_init(|| {
        let mut out: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
        let mut cur = "";
        for line in include_str!("library_units.txt").lines().filter(|l| !l.starts_with('#')) {
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                cur = name;
            } else {
                out.entry(cur).or_default().extend(line.split_whitespace());
            }
        }
        out
    });
    all.get(lib).unwrap_or_else(|| EMPTY.get_or_init(HashSet::new))
}

/// Whether the unit library `lib` declares `unit`.
pub fn declares(lib: &str, unit: &str) -> bool {
    library_names(lib).contains(unit)
}

thread_local! {
    /// Characters, unit names, imports and members read on this thread to find the units of a model.
    static WORK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn work(n: usize) {
    WORK.with(|w| w.set(w.get().saturating_add(n as u64)));
}

/// The count since the last call; it starts again at zero. A test reads it to show, with no clock, that the unit check
/// reads each text and each namespace once.
pub fn take_work_steps() -> u64 {
    WORK.with(|w| w.replace(0))
}

fn unit_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '°' | 'µ' | 'Ω' | '%')
}

/// The unit names in the brackets that follow a number in `text`: each chain of names (`SI::V`) once, in order. Brackets that
/// follow a name (`xs[1]`), and bracket contents that are only numbers, hold no unit. Each character is read a fixed
/// number of times: the text after a bracket is read only up to its `]`, and not again once no `]` is left.
pub fn unit_names(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    work(chars.len());
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut no_close = false;
    let mut i = 0;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' {
                i += 1;
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '[' => {
                let mut b = i;
                while b > 0 && chars[b - 1].is_whitespace() {
                    b -= 1;
                }
                let after_number = b > 0 && (chars[b - 1].is_ascii_digit() || chars[b - 1] == '.') && number_ends_here(&chars, b);
                let close = if after_number && !no_close { chars[i + 1..].iter().position(|c| *c == ']').map(|p| i + 1 + p) } else { None };
                if after_number && !no_close {
                    work(close.unwrap_or(chars.len()) - i);
                    no_close = close.is_none();
                }
                if let (true, Some(close)) = (after_number, close) {
                    let inner: String = chars[i + 1..close].iter().collect();
                    for tok in inner.split(|c: char| !(unit_char(c) || c == ':')) {
                        let tok = tok.trim_matches(':');
                        if tok.is_empty() || tok.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(true) {
                            continue;
                        }
                        let chain = tok.split("::").filter(|s| !s.is_empty()).collect::<Vec<_>>().join("::");
                        work(1);
                        if !chain.is_empty() && seen.insert(chain.clone()) {
                            out.push(chain);
                        }
                    }
                    i = close;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// True when the digits that end at `end` are a number, not the tail of a name such as `x1` or `a.b2`.
fn number_ends_here(chars: &[char], end: usize) -> bool {
    let mut j = end;
    while j > 0 && (chars[j - 1].is_ascii_digit() || chars[j - 1] == '.' || chars[j - 1] == '_') {
        j -= 1;
    }
    !(j > 0 && (chars[j - 1].is_alphabetic() || chars[j - 1] == '_'))
}

/// The texts of an element in which a unit may stand.
fn texts(e: &Element) -> impl Iterator<Item = &str> {
    e.value.iter().map(String::as_str).chain(["trigger", "guard", "effect"].iter().filter_map(|k| e.props.get(*k).map(String::as_str)))
}

/// A unit that does not resolve where an element writes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Gap {
    pub element: ElementId,
    pub unit: String,
}

/// The unit libraries one namespace (or the top of one file) imports itself.
#[derive(Default)]
struct Cover {
    /// `import SI::*;`: the units SI declares.
    si: bool,
    /// `import USCustomaryUnits::*;`: the units USCustomaryUnits declares (it imports SI privately).
    us: bool,
    /// `import SI::V;`: that name, and whether the library is the US customary one.
    named: HashSet<(String, bool)>,
}

impl Cover {
    fn add(&mut self, imp: &Element) {
        let Some(r) = imp.refs_of(RefRole::Import).next() else { return };
        let segs: Vec<&str> = r.segments.iter().map(|s| s.name.as_str()).collect();
        let wild = imp.props.get("wildcard").is_some_and(|w| !w.is_empty());
        match segs.as_slice() {
            [lib] if wild && UNIT_LIBRARIES.contains(lib) => {
                if *lib == "USCustomaryUnits" {
                    self.us = true;
                } else {
                    self.si = true;
                }
            }
            [lib, name] if UNIT_LIBRARIES.contains(lib) => {
                self.named.insert((name.to_string(), *lib == "USCustomaryUnits"));
            }
            _ => {}
        }
    }

    /// A unit is covered by an import of a library that declares it.
    fn covers(&self, unit: &str) -> bool {
        (self.us && declares("USCustomaryUnits", unit)) || (self.si && declares("SI", unit)) || self.named.contains(&(unit.to_string(), true)) || self.named.contains(&(unit.to_string(), false))
    }
}

/// The unit imports of the model, read once: by the namespace that holds them, and by file for those at the top of a file.
struct Scopes {
    inside: HashMap<ElementId, Cover>,
    top: HashMap<usize, Cover>,
    /// Whether a unit is imported in a namespace or one around it: each (namespace, unit) is worked out once.
    known: HashMap<(ElementId, String), bool>,
}

impl Scopes {
    fn build(model: &Model) -> Self {
        let mut s = Scopes { inside: HashMap::new(), top: HashMap::new(), known: HashMap::new() };
        work(model.elements.len());
        for e in model.elements.iter().filter(|e| e.kind == "import") {
            match e.owner {
                Some(o) => s.inside.entry(o).or_default().add(e),
                None => s.top.entry(e.file).or_default().add(e),
            }
        }
        s
    }

    /// Whether an import of a unit library is in scope for `e`: in an owner of `e`, or at the top of its file.
    fn imported(&mut self, model: &Model, e: &Element, unit: &str) -> bool {
        // Up the owners until one that gives the unit or one whose answer is known; then every namespace passed keeps the answer.
        let mut passed: Vec<ElementId> = Vec::new();
        let mut cur = e.owner;
        let mut found = false;
        while let Some(o) = cur {
            work(1);
            if let Some(k) = self.known.get(&(o, unit.to_string())) {
                found = *k;
                break;
            }
            passed.push(o);
            if self.inside.get(&o).is_some_and(|c| c.covers(unit)) {
                found = true;
                break;
            }
            cur = model.elements[o].owner;
        }
        if !found && cur.is_none() {
            found = self.top.get(&e.file).is_some_and(|c| c.covers(unit));
        }
        // A namespace below the one that gives the unit has it too; when no one gives it, none of them has it. The
        // top of the file belongs to this file only, and so does every namespace on the way.
        for o in passed {
            self.known.insert((o, unit.to_string()), found);
        }
        found
    }
}

/// Every unit of the model that does not resolve. One pass over the elements; the names and the unit imports of the model
/// are collected once.
pub fn gaps(model: &Model) -> Vec<Gap> {
    let mut names: Option<HashSet<&str>> = None;
    let mut scopes: Option<Scopes> = None;
    let mut out = Vec::new();
    for e in &model.elements {
        for t in texts(e) {
            if !t.contains('[') {
                continue;
            }
            for u in unit_names(t) {
                let names = names.get_or_insert_with(|| model.elements.iter().flat_map(|x| x.name.as_deref().into_iter().chain(x.short_name.as_deref())).collect());
                if resolves(&u, names) {
                    continue;
                }
                if u.contains("::") || !scopes.get_or_insert_with(|| Scopes::build(model)).imported(model, e, &u) {
                    out.push(Gap { element: e.id, unit: u });
                }
            }
        }
    }
    out
}

/// Whether the model or a library name gives the unit, with no import.
fn resolves(unit: &str, names: &HashSet<&str>) -> bool {
    let first = unit.split("::").next().unwrap_or(unit);
    if unit.contains("::") {
        let rest = &unit[first.len() + 2..];
        if UNIT_LIBRARIES.contains(&first) {
            return declares(first, rest);
        }
        return ["ISQ", "ScalarValues"].contains(&first) || names.contains(first);
    }
    names.contains(unit)
}

/// The root of `e`'s owner chain: where a writer puts the import so the whole package sees it.
pub fn import_home(model: &Model, e: ElementId) -> ElementId {
    std::iter::successors(Some(e), |i| model.elements[*i].owner).last().unwrap_or(e)
}

/// The import that brings `unit` into scope: `USCustomaryUnits::*` for a unit only it declares, else `SI::*`.
pub fn library_for(unit: &str) -> &'static str {
    if !declares("SI", unit) && declares("USCustomaryUnits", unit) { "USCustomaryUnits::*" } else { DEFAULT_IMPORT }
}
