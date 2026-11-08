//! Text forms the official SysML v2 textual grammar rejects and the parser reads anyway. The syntax gate reports each
//! one with its own rule id, so a file the tools write or check is also a file the official grammar accepts.
//! The scan works on tokens (comments, strings and quoted names are skipped), one pass per file.

use crate::model::Model;
use serde_json::{json, Value};

/// `(rule id, meaning)` of every rule here; the registry lists them under the syntax gate.
pub const RULES: &[(&str, &str)] = &[
    ("syntax-import-visibility", "An import has no visibility word (the grammar requires public, private or protected)"),
    ("syntax-multiplicity", "A multiplicity is an open range [1..], has no lower bound [..5], lists values [1, 2] or is written twice [1][2]"),
    ("syntax-missing-operand", "An expression or value is missing its operand: `= ;`, `1 + ;`"),
    ("syntax-real-literal", "A real number has a point and no fraction digits (`1.`)"),
    ("syntax-definition-kind", "`def` has no kind word before it (`def A;`, `use def U;`)"),
    ("syntax-repeated-modifier", "A visibility or prefix word is written twice in a row"),
    ("syntax-alias-target", "An alias has no `for` target"),
    ("syntax-assign-operator", "`assign` uses `=`; the grammar writes `:=`"),
    ("syntax-accept-payload", "`accept` names nothing to accept"),
    ("syntax-enum-member", "An enumeration body holds a member that is not an enumerated value"),
    ("syntax-empty-transition", "A transition has no source or target"),
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Num(String),
    Text,
    Quoted,
    P(&'static str),
    Other(char),
}

struct T {
    tok: Tok,
    at: usize,
}

/// The tokens of `src`. `steps` counts every loop turn, so a test can show the cost is linear without a clock.
fn lex(src: &str, steps: &mut u64) -> Vec<T> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    const MULTI: [&str; 14] = ["::>", ">>", "::", "..", ":=", ":>", "==", "!=", "<=", ">=", "->", "**", "=>", "[]"];
    while i < b.len() {
        *steps += 1;
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if b[i..].starts_with(b"//*") {
            // `//* ... */` is a block note; it is not part of the model.
            i += 3;
            while i < b.len() && !b[i..].starts_with(b"*/") {
                *steps += 1;
                i += 1;
            }
            i = (i + 2).min(b.len());
        } else if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                *steps += 1;
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            i += 2;
            while i < b.len() && !b[i..].starts_with(b"*/") {
                *steps += 1;
                i += 1;
            }
            i = (i + 2).min(b.len());
        } else if c == b'"' || c == b'\'' {
            let at = i;
            i += 1;
            while i < b.len() && b[i] != c {
                *steps += 1;
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
            out.push(T { tok: if c == b'"' { Tok::Text } else { Tok::Quoted }, at });
        } else if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let at = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
                *steps += 1;
                i += 1;
            }
            out.push(T { tok: Tok::Word(src[at..i].to_string()), at });
        } else if c.is_ascii_digit() {
            let at = i;
            // A fraction, or an exponent, belongs to the number; `1..5` is a range.
            i = crate::lexer::number_end(b, i);
            *steps += (i - at) as u64;
            out.push(T { tok: Tok::Num(src[at..i].to_string()), at });
        } else {
            let at = i;
            if let Some(m) = MULTI.iter().find(|m| b[i..].starts_with(m.as_bytes())) {
                i += m.len();
                out.push(T { tok: Tok::P(m), at });
            } else {
                let ch = src[i..].chars().next().unwrap_or(' ');
                i += ch.len_utf8();
                const ONE: [&str; 22] = [";", "{", "}", "[", "]", "(", ")", ",", ".", ":", "=", "+", "-", "*", "/", "^", "<", ">", "#", "@", "~", "?"];
                match ONE.iter().find(|s| s.starts_with(ch)) {
                    Some(s) => out.push(T { tok: Tok::P(s), at }),
                    None => out.push(T { tok: Tok::Other(ch), at }),
                }
            }
        }
    }
    out
}

fn is_word(t: &T, w: &str) -> bool {
    matches!(&t.tok, Tok::Word(x) if x == w)
}

fn is_p(t: &T, p: &str) -> bool {
    matches!(&t.tok, Tok::P(x) if *x == p)
}

const VISIBILITY: [&str; 3] = ["public", "private", "protected"];
const PREFIXES: [&str; 14] = ["public", "private", "protected", "abstract", "variation", "readonly", "derived", "ref", "end", "in", "out", "inout", "individual", "constant"];
/// Words that start a declaration, and so cannot be a member of an enumeration.
const DECLARATIONS: [&str; 30] = [
    "part", "attribute", "item", "port", "action", "state", "requirement", "constraint", "calc", "connection", "interface", "allocation", "flow", "view", "viewpoint", "rendering",
    "verification", "analysis", "case", "concern", "occurrence", "event", "message", "binding", "succession", "transition", "satisfy", "perform", "exhibit", "include",
];

/// One finding: file index, byte offset, rule id, message, fix.
pub struct Hit {
    pub file: usize,
    pub at: usize,
    pub rule: &'static str,
    pub message: String,
    pub fix: String,
}

/// What the scan looks up ahead of a token, built in one pass each so that no token starts a walk of its own.
struct Ahead {
    /// For a `[`: the index after its matching `]`, or the token count when it is never closed.
    close: Vec<usize>,
    /// Commas among the tokens before each index.
    commas: Vec<u32>,
    /// Tokens before each index that are not a number, `..` or `*`.
    non_numeric: Vec<u32>,
    /// The nearest index at or after each index of: a statement end (`;`, `{`, `}`), the word `for`, the first of `;` `{` `=` `:=`.
    stmt_end: Vec<usize>,
    word_for: Vec<usize>,
    assign_stop: Vec<usize>,
}

fn ahead(t: &[T], steps: &mut u64) -> Ahead {
    let n = t.len();
    let mut a = Ahead { close: vec![n; n], commas: vec![0; n + 1], non_numeric: vec![0; n + 1], stmt_end: vec![n; n + 1], word_for: vec![n; n + 1], assign_stop: vec![n; n + 1] };
    let mut open: Vec<usize> = Vec::new();
    for (i, k) in t.iter().enumerate() {
        *steps += 1;
        if is_p(k, "[") {
            open.push(i);
        } else if is_p(k, "]") {
            if let Some(o) = open.pop() {
                a.close[o] = i + 1;
            }
        }
        a.commas[i + 1] = a.commas[i] + is_p(k, ",") as u32;
        let numeric = matches!(&k.tok, Tok::Num(_)) || is_p(k, "..") || is_p(k, "*");
        a.non_numeric[i + 1] = a.non_numeric[i] + !numeric as u32;
    }
    for i in (0..n).rev() {
        *steps += 1;
        let k = &t[i];
        a.stmt_end[i] = if is_p(k, ";") || is_p(k, "{") || is_p(k, "}") { i } else { a.stmt_end[i + 1] };
        a.word_for[i] = if is_word(k, "for") { i } else { a.word_for[i + 1] };
        a.assign_stop[i] = if is_p(k, ";") || is_p(k, "{") || is_p(k, "=") || is_p(k, ":=") { i } else { a.assign_stop[i + 1] };
    }
    a
}

fn scan(file: usize, src: &str, out: &mut Vec<Hit>, steps: &mut u64) {
    let t = lex(src, steps);
    let look = ahead(&t, steps);
    let mut hit = |at: usize, rule: &'static str, message: &str, fix: &str| out.push(Hit { file, at, rule, message: message.into(), fix: fix.into() });
    // The first token of the statement each token belongs to, and what the braces opened by `enum def` contain.
    let mut stmt_start = true;
    let mut stack: Vec<bool> = Vec::new();
    let mut header_enum = false;
    // The words of this statement so far: all of them prefix words, one of them a visibility word, one of them `assign`.
    let (mut lead, mut has_visibility, mut has_assign) = (true, false, false);
    let mut i = 0;
    while i < t.len() {
        *steps += 1;
        let k = &t[i];
        let prev = i.checked_sub(1).map(|j| &t[j]);
        let next = t.get(i + 1);
        let first = stmt_start;
        stmt_start = false;
        match &k.tok {
            Tok::P(";") | Tok::P("{") | Tok::P("}") => {
                match &k.tok {
                    Tok::P("{") => stack.push(header_enum),
                    Tok::P("}") => {
                        stack.pop();
                    }
                    _ => {}
                }
                stmt_start = true;
                header_enum = false;
                (lead, has_visibility, has_assign) = (true, false, false);
            }
            Tok::Word(w) => {
                let w = w.as_str();
                // A declaration may start with visibility and prefix words; `lead` is true until the first other word.
                if w == "import" && lead && !has_visibility {
                    hit(k.at, "syntax-import-visibility", "an import without public, private or protected is not valid SysML v2", "Write `private import X::*;` (private is the default meaning), or `public import` to re-export.");
                }
                if PREFIXES.contains(&w) && prev.is_some_and(|p| is_word(p, w)) {
                    hit(k.at, "syntax-repeated-modifier", &format!("`{w}` is written twice in a row"), "Remove the repeated word.");
                }
                if w == "def" {
                    let bad = match prev {
                        None => true,
                        Some(p) => match &p.tok {
                            Tok::P(";") | Tok::P("{") | Tok::P("}") => true,
                            // `individual def A;` is an occurrence definition; every other prefix word needs a kind after it.
                            Tok::Word(x) => (PREFIXES.contains(&x.as_str()) && x != "individual") || x == "use",
                            _ => false,
                        },
                    };
                    if bad {
                        hit(k.at, "syntax-definition-kind", "`def` needs a kind word before it, such as `part def`", "Write the kind: `part def A;`, `use case def U;`.");
                    }
                }
                if w == "enum" && next.is_some_and(|n| is_word(n, "def")) {
                    header_enum = true;
                }
                if w == "alias" && lead && look.word_for[i + 1] >= look.stmt_end[i + 1] {
                    hit(k.at, "syntax-alias-target", "an alias needs a target: `alias B for A;`", "Write `alias B for A;`.");
                }
                if w == "assign" && lead && !has_assign {
                    // `assign v = 1;` has `=` before `:=` and `;`.
                    let j = look.assign_stop[i + 1];
                    if j < t.len() && is_p(&t[j], "=") {
                        hit(t[j].at, "syntax-assign-operator", "an assignment is written with `:=`, not `=`", "Write `assign v := 1;`.");
                    }
                }
                if w == "accept" && next.is_some_and(|n| is_p(n, ";")) {
                    hit(k.at, "syntax-accept-payload", "`accept` needs something to accept", "Write `accept SignalName;` or `accept after 5 [s];`.");
                }
                if w == "transition" && next.is_some_and(|n| is_p(n, ";")) {
                    hit(k.at, "syntax-empty-transition", "a transition needs a source and a target", "Write `transition first a then b;`, or remove the transition.");
                }
                if first && stack.last() == Some(&true) && DECLARATIONS.contains(&w) {
                    hit(k.at, "syntax-enum-member", &format!("an enumeration holds enumerated values, not a `{w}`"), "Write the value as `name;` or `enum name;`, and move other members out of the enum.");
                }
                has_visibility |= VISIBILITY.contains(&w);
                has_assign |= w == "assign";
                lead &= PREFIXES.contains(&w);
            }
            Tok::P("[") => {
                // The contents up to the matching `]`, read from the counts taken before the scan.
                let j = look.close[i];
                let (a, b) = (i + 1, j.saturating_sub(1).max(i + 1));
                let len = b - a;
                let open_range = len >= 2 && is_p(&t[b - 1], "..");
                let no_lower = len >= 1 && is_p(&t[a], "..");
                let list = look.commas[b] > look.commas[a];
                let numeric = len >= 1 && look.non_numeric[b] == look.non_numeric[a];
                let twice = numeric && t.get(j).is_some_and(|x| is_p(x, "["));
                if open_range || no_lower || list || twice {
                    hit(k.at, "syntax-multiplicity", "a multiplicity is [n], [n..m] or [n..*]; this one is open, has no lower bound or lists values", "Write [1..5], [0..*] or [3].");
                }
            }
            Tok::Num(n) => {
                if next.is_some_and(|x| is_p(x, ".") && x.at == k.at + n.len()) && !n.contains('.') && !n.contains(['e', 'E']) {
                    let after = t.get(i + 2);
                    // `1.` followed by a name is a feature access on a number; anything else is a real without fraction digits.
                    if !after.is_some_and(|a| matches!(a.tok, Tok::Word(_)) && a.at == k.at + n.len() + 1) {
                        hit(k.at, "syntax-real-literal", "a real number needs digits after the point", "Write `1.0`, or `1` for an integer.");
                    }
                }
            }
            _ => {}
        }
        // Operands: `= ;`, `= )`, `1 + ;`.
        if let Tok::P(p) = &k.tok {
            let binary = matches!(*p, "=" | ":=" | "+" | "-" | "/" | "^" | "**" | "==" | "!=" | "<=" | ">=" | "*" | "->");
            // `A::*` and `A::**` are import wildcards, `[1..*]` is a multiplicity, `x ** *`... is not an operand either.
            let star_ok = (*p == "*" && prev.is_some_and(|x| is_p(x, "::") || is_p(x, "..") || is_p(x, "**"))) || (*p == "**" && prev.is_some_and(|x| is_p(x, "::")));
            let closes = next.is_none_or(|n| matches!(&n.tok, Tok::P(";") | Tok::P(")") | Tok::P("]") | Tok::P("}") | Tok::P(",")));
            if binary && !star_ok && closes && !(*p == "*" && next.is_some_and(|n| is_p(n, "]"))) {
                hit(k.at, "syntax-missing-operand", &format!("`{p}` has nothing after it"), "Write the value or operand after the operator, or remove the statement.");
            }
        }
        i += 1;
    }
}

/// How many loop turns the scan of `text` takes. A test reads this to show the cost grows in step with the text, with no clock.
pub fn work_steps(text: &str) -> u64 {
    let mut steps = 0;
    let mut hits = Vec::new();
    scan(0, text, &mut hits, &mut steps);
    let starts = line_starts(text, &mut steps);
    for h in &hits {
        line_of(&starts, h.at, &mut steps);
    }
    steps
}

/// The byte offset at which each line starts.
fn line_starts(text: &str, steps: &mut u64) -> Vec<usize> {
    let mut v = vec![0];
    for (i, b) in text.bytes().enumerate() {
        *steps += 1;
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

/// The 1-based line of byte offset `at`: a binary search, so a file with many findings costs no pass per finding.
fn line_of(starts: &[usize], at: usize, steps: &mut u64) -> usize {
    let (mut lo, mut hi) = (0, starts.len());
    while hi - lo > 1 {
        *steps += 1;
        let mid = (lo + hi) / 2;
        if starts[mid] <= at {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo + 1
}

/// The grammar hits of every file of `model`, as syntax-gate findings.
pub fn findings(model: &Model) -> Vec<Value> {
    let mut out = Vec::new();
    let mut steps = 0;
    for (i, f) in model.files.iter().enumerate() {
        let mut hits = Vec::new();
        scan(i, &f.text, &mut hits, &mut steps);
        if hits.is_empty() {
            continue;
        }
        let starts = line_starts(&f.text, &mut steps);
        for h in hits {
            out.push(json!({
                "gate": "syntax", "rule": h.rule, "severity": "error", "element": null,
                "file": f.path, "line": line_of(&starts, h.at.min(f.text.len()), &mut steps),
                "message": h.message,
                "fix": h.fix,
            }));
        }
    }
    out
}
