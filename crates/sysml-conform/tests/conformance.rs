//! Acceptance tests for the official-grammar checker. They need only the files
//! vendored in this crate.

use std::path::{Path, PathBuf};
use std::process::Command;
use sysml_conform::grammar::Sources;
use sysml_conform::{
    Checker, Limits, Verdict, CORRECTIONS, GRAMMAR_RELEASE, KERML_GRAMMAR, SYSML_GRAMMAR,
};

fn dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

fn sysml_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(next) = stack.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "sysml") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn verdict(checker: &Checker, text: &str) -> Verdict {
    checker.check_str(text, &Limits::default()).verdict
}

/// The corrections file without the block that defines `rule`.
fn corrections_without(rule: &str) -> String {
    let blocks: Vec<&str> = CORRECTIONS.split("\n\n").collect();
    let kept: Vec<&str> = blocks
        .iter()
        .filter(|block| {
            !block.lines().any(|line| {
                line.strip_prefix(rule).is_some_and(|rest| {
                    rest.trim_start().starts_with(':') || rest.trim_start().starts_with('=')
                })
            })
        })
        .copied()
        .collect();
    assert_eq!(
        kept.len() + 1,
        blocks.len(),
        "exactly one block defines {rule}"
    );
    kept.join("\n\n")
}

#[test]
fn grammar_is_assembled_from_the_vendored_files() {
    let checker = Checker::new();
    let report = &checker.grammar.report;
    assert!(
        report.sysml_rules.len() > 300,
        "SysML rules: {}",
        report.sysml_rules.len()
    );
    // The KerML file supplies expressions, literals and qualified names only.
    assert!(report.kerml_rules.contains(&"OwnedExpression".to_string()));
    assert!(report.kerml_rules.contains(&"QualifiedName".to_string()));
    for kerml_only in [
        "Feature",
        "Classifier",
        "TypeBody",
        "FunctionBodyPart",
        "MetadataFeature",
    ] {
        assert!(
            !report.kerml_rules.contains(&kerml_only.to_string()),
            "{kerml_only} must not be reachable"
        );
    }
    assert_eq!(report.unmatchable_words, Vec::<String>::new());
    assert_eq!(report.corrections.len(), 14);
    for fix in &report.corrections {
        assert!(fix.replaces, "{} must replace an official rule", fix.rule);
        assert!(fix.reason.len() > 40, "{} needs a written reason", fix.rule);
    }
    let notice =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("grammar/NOTICE.md"))
            .unwrap();
    assert!(notice.contains(&format!("`{GRAMMAR_RELEASE}`")));
    assert!(
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("grammar/LICENSE"))
            .unwrap()
            .starts_with("Eclipse Public License - v 2.0")
    );
}

#[test]
fn official_sample_files_are_valid() {
    let checker = Checker::new();
    let files = sysml_files(&dir("official"));
    assert_eq!(files.len(), 42);
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            verdict(&checker, &text),
            Verdict::Valid,
            "{}",
            path.display()
        );
    }
}

#[test]
fn known_bad_files_are_invalid_at_the_right_place() {
    let checker = Checker::new();
    let manifest = std::fs::read_to_string(dir("bad").join("expected.tsv")).unwrap();
    let mut listed = Vec::new();
    for row in manifest
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let cols: Vec<&str> = row.split('\t').collect();
        let (file, line, column, found) = (
            cols[0],
            cols[1].parse::<u32>().unwrap(),
            cols[2].parse::<u32>().unwrap(),
            cols[3],
        );
        let text = std::fs::read_to_string(dir("bad").join(file)).unwrap();
        match verdict(&checker, &text) {
            Verdict::Invalid(fault) => {
                assert_eq!(
                    (fault.line, fault.column, fault.found.as_str()),
                    (line, column, found),
                    "{file}"
                );
                assert!(
                    !fault.allowed.is_empty(),
                    "{file}: the allowed set is empty"
                );
                assert!(
                    !fault.allowed.contains(&format!("'{found}'")),
                    "{file}: found token is in the allowed set"
                );
            }
            other => panic!("{file}: expected invalid, got {other:?}"),
        }
        listed.push(dir("bad").join(file));
    }
    assert!(listed.len() >= 30);
    listed.sort();
    assert_eq!(
        listed,
        sysml_files(&dir("bad")),
        "every bad file is in expected.tsv"
    );
}

#[test]
fn allowed_set_names_the_tokens_the_grammar_permits() {
    let checker = Checker::new();
    let Verdict::Invalid(fault) =
        verdict(&checker, "package P {\n    part def A;\n    alias B;\n}\n")
    else {
        panic!("expected invalid");
    };
    assert_eq!(fault.allowed, vec!["'for'".to_string()]);
    assert_eq!(
        (fault.line, fault.column, fault.found_kind.as_str()),
        (3, 12, "symbol")
    );
    let Verdict::Invalid(fault) = verdict(&checker, "package P {\n    part x [1, 2];\n}\n") else {
        panic!("expected invalid");
    };
    assert_eq!(fault.allowed, vec!["'.'", "'..'", "']'"]);
}

/// Each correction must change a verdict; one that changes nothing would be a
/// silent second grammar. Most are forced by a vendored official file.
#[test]
fn every_correction_is_needed() {
    let samples: Vec<(PathBuf, String)> = sysml_files(&dir("official"))
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            (p, text)
        })
        .collect();
    // Corrections no vendored official file forces: a text and the verdict the
    // corrected grammar must give, which the uncorrected rule does not give.
    let probes: [(&str, &str, bool); 3] = [
        ("OwnedCrossFeature", "part def A { end ref part x; }", false),
        (
            "AnnotatingElement",
            "metadata def M; part x { #M @ m defined by M; }",
            true,
        ),
        (
            "MetadataUsageDeclaration",
            "metadata def M; part x { @ m defined by M; }",
            true,
        ),
    ];
    let full = Checker::new();
    for fix in &full.grammar.report.corrections {
        let mut corrections = corrections_without(&fix.rule);
        if fix.rule == "AnnotatingElement" {
            // The official rule never reaches MetadataUsage, so the correction to
            // its declaration has to go as well.
            corrections = corrections
                .split("\n\n")
                .filter(|block| !block.contains("\nMetadataUsageDeclaration"))
                .collect::<Vec<_>>()
                .join("\n\n");
        }
        let without = Checker::from_sources(&Sources {
            sysml: SYSML_GRAMMAR,
            kerml: KERML_GRAMMAR,
            corrections: &corrections,
        })
        .unwrap_or_else(|e| panic!("grammar without {}: {e}", fix.rule));
        if let Some((_, text, valid)) = probes.iter().find(|(rule, _, _)| *rule == fix.rule) {
            assert_eq!(
                verdict(&full, text) == Verdict::Valid,
                *valid,
                "{} probe, corrected grammar",
                fix.rule
            );
            assert_eq!(
                verdict(&without, text) == Verdict::Valid,
                !*valid,
                "{} probe, official rule",
                fix.rule
            );
            continue;
        }
        let broken = samples
            .iter()
            .filter(|(_, text)| verdict(&without, text) != Verdict::Valid)
            .count();
        assert!(
            broken > 0,
            "no vendored official file needs the correction to {}",
            fix.rule
        );
    }
}

#[test]
fn a_text_over_a_limit_is_not_checked() {
    let checker = Checker::new();
    let text = std::fs::read_to_string(dir("official").join("library/Parts.sysml")).unwrap();
    assert_eq!(verdict(&checker, &text), Verdict::Valid);
    let cases = [
        Limits {
            max_tokens: 20,
            ..Limits::default()
        },
        Limits {
            steps_base: 200,
            steps_per_byte: 0,
            ..Limits::default()
        },
        Limits {
            max_items: 3,
            ..Limits::default()
        },
    ];
    for limits in cases {
        let report = checker.check_str(&text, &limits);
        assert!(
            matches!(report.verdict, Verdict::NotChecked(_)),
            "{limits:?} gave {:?}",
            report.verdict
        );
    }
    assert!(matches!(
        checker
            .check_bytes(&[b'p', 0xff, 0xfe], &Limits::default())
            .verdict,
        Verdict::NotChecked(_)
    ));
}

/// The official expression rules are flat (`A = A op A`), and a recogniser that
/// runs them as written takes steps with the cube of the length. The step limit
/// of one text stops that; the answer is then neither "valid" nor "invalid".
#[test]
fn the_work_budget_bounds_a_costly_expression() {
    let plain = Checker::plain();
    let text = format!(
        "package P {{ attribute x = {}; }}\n",
        vec!["1"; 3000].join(" + ")
    );
    let limits = Limits {
        steps_base: 20_000_000,
        steps_per_byte: 0,
        ..Limits::default()
    };
    let report = plain.check_str(&text, &limits);
    assert!(
        matches!(report.verdict, Verdict::NotChecked(_)),
        "{:?}",
        report.verdict
    );
    assert!(
        report.steps < limits.steps_base + 100_000,
        "steps {}",
        report.steps
    );
    // The limit of one text is in proportion to its size: with the default
    // limits the same recogniser stops after 200 steps a byte.
    let report = plain.check_str(&text, &Limits::default());
    assert!(matches!(report.verdict, Verdict::NotChecked(_)));
    let allowed = Limits::default().step_limit(text.len());
    assert_eq!(allowed, 100_000 + 200 * text.len() as u64);
    assert!(report.steps <= allowed + 10_000, "steps {}", report.steps);
    // A low limit for one text stops early.
    let low = Limits {
        steps_base: 5_000,
        steps_per_byte: 0,
        ..Limits::default()
    };
    let checker = Checker::new();
    let report = checker.check_str(&text, &low);
    assert!(matches!(report.verdict, Verdict::NotChecked(_)));
    assert!(report.steps <= 15_000, "steps {}", report.steps);
    // The checker in use reads the same text in step with its length.
    let report = checker.check_str(&text, &Limits::default());
    assert_eq!(report.verdict, Verdict::Valid);
    assert!(report.steps < 400_000, "steps {}", report.steps);
}

/// Texts that repeat one form `k` times. Each line: a name, what comes before,
/// the repeated unit, what comes after.
const REPEATED_FORMS: &[(&str, &str, &str, &str)] = &[
    ("sum", "package P { attribute x = 1", " + 1", "; }"),
    ("feature chain", "package P { attribute x = a", ".a", "; }"),
    ("power", "package P { attribute x = 1", " ** 1", "; }"),
    ("and", "package P { attribute x = a", " and a", "; }"),
    (
        "mixed operators",
        "package P { attribute x = 1",
        " + -1 * 2 == 3 or not a",
        "; }",
    ),
    (
        "sum of conditionals",
        "package P { attribute x = 1",
        " + if a ? 1 else 1",
        "; }",
    ),
    ("classification", "package P { attribute x = a", " istype T", "; }"),
    ("cast in a sum", "package P { attribute x = a", " as T + a", "; }"),
    ("unary run", "package P { attribute x = ", "- ", "1; }"),
    (
        "conditional run",
        "package P { attribute x = ",
        "if a ? 1 else ",
        "2; }",
    ),
    ("index run", "package P { attribute x = a", "[1]", "; }"),
    ("call chain", "package P { attribute x = a", ".a->f(1)", "; }"),
    ("arrow run", "package P { attribute x = a", "->f(1)", "; }"),
    ("qualified name", "package P { attribute x = a", "::a", "; }"),
    ("arguments", "package P { attribute x = f(", "1, ", "1); }"),
    ("sequence", "package P { attribute x = (1", ", 1", "); }"),
    ("members", "package P {", " part def A { attribute x = 1; }", " }"),
    (
        "else if run",
        "package P { action def A { ",
        "if a { } else ",
        "{ } } }",
    ),
    (
        "successions",
        "package P { action def A { action a; ",
        "then a; ",
        "} }",
    ),
    (
        "transitions",
        "package P { state def S { state a; ",
        "transition first a if x > 1 do b then a; ",
        "} }",
    ),
    (
        "flow end chain",
        "package P { part x { flow from a",
        ".a",
        ".i to b.i; } }",
    ),
    ("metadata prefixes", "package P { ", "#M ", "part x; }"),
    (
        "import filters",
        "package P { private import A::*",
        "[@T]",
        "; }",
    ),
    (
        "specialization list",
        "package P { part def A :> B",
        ", B",
        "; }",
    ),
];

fn repeated(form: &(&str, &str, &str, &str), k: usize) -> String {
    format!("{}{}{}\n", form.1, form.2.repeat(k), form.3)
}

/// No clock: the recogniser counts its steps. Twice the text may take a little
/// more than twice the steps and no more.
#[test]
fn work_grows_in_step_with_the_text() {
    let checker = Checker::new();
    let free = Limits {
        steps_per_byte: u64::MAX,
        ..Limits::default()
    };
    for form in REPEATED_FORMS {
        let small = checker.check_str(&repeated(form, 400), &free);
        let large = checker.check_str(&repeated(form, 800), &free);
        assert_eq!(small.verdict, Verdict::Valid, "{}", form.0);
        assert_eq!(large.verdict, Verdict::Valid, "{}", form.0);
        assert!(
            large.steps * 10 <= small.steps * 21,
            "{}: {} steps for 400, {} for 800",
            form.0,
            small.steps,
            large.steps
        );
        // And every form is far inside the limit of a text of its size.
        let text = repeated(form, 800);
        assert!(
            large.steps * 2 < Limits::default().step_limit(text.len()),
            "{}: {} steps for {} bytes",
            form.0,
            large.steps,
            text.len()
        );
    }
    // The rules as the files give them do not have this property: that is what
    // the rewrites of `Grammar::compile` are for. Four forms show it.
    let plain = Checker::plain();
    let mut slow = Vec::new();
    for form in REPEATED_FORMS {
        let small = plain.check_str(&repeated(form, 100), &free);
        let large = plain.check_str(&repeated(form, 200), &free);
        if large.steps > small.steps * 3 {
            slow.push(form.0);
        }
    }
    for name in ["sum", "feature chain", "sequence", "else if run", "import filters"] {
        assert!(slow.contains(&name), "{name} is not slow as written: {slow:?}");
    }
    assert_eq!(checker.grammar.report.rewrites.len(), 6);
}

/// A small generator with a fixed seed: no dependency, the same texts each run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The rewrites of `Grammar::compile` must change no verdict, no fault place and
/// no "allowed" list. The rules as written and the rewritten rules read the
/// vendored files, the known-bad files and 30 000 made texts; every answer is
/// compared in full.
#[test]
fn the_rewritten_rules_give_the_same_answers() {
    let (fast, plain) = (Checker::new(), Checker::plain());
    assert!(plain.grammar.report.rewrites.is_empty());
    let free = Limits {
        steps_per_byte: u64::MAX,
        ..Limits::default()
    };
    let same = |text: &str| {
        assert_eq!(
            fast.check_str(text, &free).verdict,
            plain.check_str(text, &free).verdict,
            "{text}"
        );
    };
    for path in sysml_files(&dir("official"))
        .into_iter()
        .chain(sysml_files(&dir("bad")))
    {
        same(&std::fs::read_to_string(&path).unwrap());
    }
    const WORDS: &[&str] = &[
        "a", "b", "T", "1", "2.5", "\"s\"", "+", "-", "*", "/", "**", "^", "==", "!=", "<", ">=",
        "and", "or", "xor", "implies", "not", "~", "??", "..", "if", "?", "else", "(", ")", "[",
        "]", "{", "}", ".", ",", "::", "->", ".?", "#", "istype", "hastype", "as", "@", "@@",
        "meta", "all", "new", "null", "true", "=", ";", "in", "x", "f", "metadata", "$", "|", "&",
        "%", "===", ":", "then", "action", "part", "accept", "via", "do", "first", "send", "to",
        "assign", ":=", "while", "loop", "until", "for", "decide", "merge",
    ];
    const FRAMES: &[(&str, &str)] = &[
        ("package P { attribute x = ", "; }"),
        ("package P { part p [", "]; }"),
        ("package P { calc def C { ", " } }"),
        ("package P { action def A { if ", " { } } }"),
        ("package P { action def A { ", " } }"),
        ("package P { state def S { transition first a if ", " then b; } }"),
        ("package P { part x { flow from ", " to b.i; } }"),
        ("package P { requirement r { require constraint { ", " } } }"),
        ("package P { private import A::*", "; }"),
        ("package P { private import A::*[@T]", "[a]; }"),
    ];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    // Token soup: most texts are invalid, at every possible place.
    for _ in 0..15_000 {
        let frame = FRAMES[rng.below(FRAMES.len())];
        let n = 1 + rng.below(12);
        let body: Vec<&str> = (0..n).map(|_| WORDS[rng.below(WORDS.len())]).collect();
        same(&format!("{}{}{}", frame.0, body.join(" "), frame.1));
    }
    // Texts made by the expression rules, most of them valid, then the same with
    // one word taken out, put in or changed.
    fn expression(rng: &mut Rng, depth: usize, out: &mut Vec<&'static str>) {
        let pick = if depth == 0 { rng.below(4) } else { rng.below(22) };
        match pick {
            0 => out.push("a"),
            1 => out.push("1"),
            2 => out.extend(["a", ".", "b"]),
            3 => out.extend(["a", "::", "b"]),
            4..=7 => {
                expression(rng, depth - 1, out);
                out.push(["+", "*", "==", "and", "or", "**", "..", "??", "implies", "xor", "<"][rng.below(11)]);
                expression(rng, depth - 1, out);
            }
            8 => {
                out.push(["-", "not", "~", "+"][rng.below(4)]);
                expression(rng, depth - 1, out);
            }
            9 => {
                out.push("if");
                expression(rng, depth - 1, out);
                out.push("?");
                expression(rng, depth - 1, out);
                out.push("else");
                expression(rng, depth - 1, out);
            }
            10 => {
                out.push("(");
                expression(rng, depth - 1, out);
                for _ in 0..rng.below(3) {
                    out.push(",");
                    expression(rng, depth - 1, out);
                }
                out.push(")");
            }
            11 => {
                expression(rng, depth - 1, out);
                out.extend([["istype", "hastype", "as", "@"][rng.below(4)], "T"]);
            }
            12 => {
                expression(rng, depth - 1, out);
                out.push("[");
                expression(rng, depth - 1, out);
                out.push("]");
            }
            13 => {
                out.extend(["f", "("]);
                for i in 0..rng.below(3) {
                    if i > 0 {
                        out.push(",");
                    }
                    expression(rng, depth - 1, out);
                }
                out.push(")");
            }
            14 => {
                expression(rng, depth - 1, out);
                out.extend([".", "b"]);
                for _ in 0..rng.below(3) {
                    out.extend([".", "a"]);
                }
            }
            15 => {
                expression(rng, depth - 1, out);
                out.extend(["->", "f", "("]);
                expression(rng, depth - 1, out);
                out.push(")");
            }
            16 => {
                expression(rng, depth - 1, out);
                out.extend([[".?", "."][rng.below(2)], "{", "in", "x", ";"]);
                expression(rng, depth - 1, out);
                out.push("}");
            }
            17 => out.extend(["all", "T"]),
            18 => out.extend(["a", "@@", "T"]),
            19 => out.extend(["new", "T", "(", ")"]),
            20 => {
                expression(rng, depth - 1, out);
                out.extend(["#", "("]);
                expression(rng, depth - 1, out);
                out.push(")");
            }
            _ => out.extend(["istype", "T"]),
        }
    }
    let mut valid = 0;
    for round in 0..15_000 {
        let frame = FRAMES[rng.below(4)];
        let mut words = Vec::new();
        let depth = 1 + rng.below(4);
        expression(&mut rng, depth, &mut words);
        if words.len() > 60 {
            continue;
        }
        match round % 4 {
            1 => {
                let at = rng.below(words.len());
                words.remove(at);
            }
            2 => {
                let at = rng.below(words.len() + 1);
                words.insert(at, WORDS[rng.below(WORDS.len())]);
            }
            3 => {
                let at = rng.below(words.len());
                words[at] = WORDS[rng.below(WORDS.len())];
            }
            _ => {}
        }
        let text = format!("{}{}{}", frame.0, words.join(" "), frame.1);
        if fast.check_str(&text, &free).verdict == Verdict::Valid {
            valid += 1;
        }
        same(&text);
    }
    assert!(valid > 3_000, "only {valid} made texts were valid");
}

/// Places where the official grammar is wider than a reader may expect. They are
/// recorded so a grammar update that changes them is seen.
#[test]
fn recorded_width_of_the_official_grammar() {
    let checker = Checker::new();
    for text in [
        "",
        "// only a note\n",
        "package P { part def A { }; }",
        "package P { part def; }",
    ] {
        assert_eq!(verdict(&checker, text), Verdict::Valid, "{text:?}");
    }
}

#[test]
fn command_line_exit_codes_and_json() {
    let exe = env!("CARGO_BIN_EXE_sysml-conform");
    let run = |args: &[&str]| {
        let out = Command::new(exe).args(args).output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
        )
    };
    let official = dir("official");
    let bad = dir("bad");
    let (code, text) = run(&[official.to_str().unwrap()]);
    assert_eq!(code, 0, "{text}");
    assert!(
        text.ends_with("42 files: 42 valid, 0 invalid, 0 not checked\n"),
        "{text}"
    );

    let (code, text) = run(&["--json", bad.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert!(text.contains("\"status\": \"invalid\", \"line\": 3, \"column\": 17, \"found\": \"part\", \"found_kind\": \"keyword\", \"allowed\": ["), "{text}");
    assert!(
        text.contains(
            "\"summary\": {\"files\": 46, \"valid\": 0, \"invalid\": 46, \"not_checked\": 0}"
        ),
        "{text}"
    );

    let (code, text) = run(&[
        "--max-tokens",
        "10",
        official.join("library/Parts.sysml").to_str().unwrap(),
    ]);
    assert_eq!(code, 2);
    assert!(text.contains("not checked: file has more than"), "{text}");
    // There is no file size limit or work budget option any more.
    let (code, _) = run(&["--max-bytes", "10", official.join("library/Parts.sysml").to_str().unwrap()]);
    assert_eq!(code, 2, "an unknown option is a usage error");
    let (code, _) = run(&[official.join("no-such-file.sysml").to_str().unwrap()]);
    assert_eq!(code, 2);
    let (code, _) = run(&[]);
    assert_eq!(code, 2);
}
