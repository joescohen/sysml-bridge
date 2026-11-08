//! Tools round 16b: the open faults of the third security review, each with a test that fails without its fix.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::rules;
use sysml_core::agent::{self, grammar, intent, trace};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    let mut m = api::to_model(f.to_vec());
    agent::add_namespace_checks(&mut m);
    m
}

fn sel<'a>() -> rules::Selection<'a> {
    rules::Selection { gates: None, rules: None, groups: None, require: None }
}

fn gate(f: &[FileIn], g: &str) -> Value {
    trace::validate_model_sel(&model(f), Some(g), &sel()).unwrap()
}

fn sysml_files_under(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            sysml_files_under(&p, out);
        } else if p.extension().map(|x| x == "sysml").unwrap_or(false) {
            out.push(p);
        }
    }
}

// ------------------------------------------------------------------ fault 1: no pair is built without a cap

// ------------------------------------------------------------------ fault 2: no edit re-points a reference that was there

/// The fixture of the review: `satisfy r by s;` inside `Car` names the outer `r` and `s`; `part engine : Engine` names the
/// outer `Engine`.
const CAP: &str = "package P {\n    part def Engine {\n        attribute mass = 1;\n    }\n    requirement def RD;\n    requirement r : RD;\n    part s;\n    part def Other;\n    part def Car {\n        part engine : Engine;\n        attribute x = 1;\n        satisfy r by s;\n        part def Inner {\n            part e2 : Engine;\n        }\n    }\n    package Q {\n        part def Engine2;\n    }\n}\n";

/// What each reference of the model resolves to: (holder, text) -> target.
fn targets(f: &[FileIn]) -> Vec<(String, String, Option<String>)> {
    let m = model(f);
    let mut v: Vec<(String, String, Option<String>)> = m.elements.iter().flat_map(|e| e.refs.iter().map(|r| (e.qualified_name.clone(), r.text.clone(), r.resolved().map(|t| m.elements[t].qualified_name.clone())))).collect();
    v.sort();
    v
}

#[test]
fn r16b_f2_a_create_a_rename_a_short_name_or_an_alias_that_captures_a_reference_is_refused() {
    let f = files(CAP);
    let before = targets(&f);
    assert!(before.contains(&("P::Car::$3".into(), "r".into(), Some("P::r".into()))), "{before:?}");
    for op in [
        // A new member named like the outer element the satisfy names.
        json!({"op": "create_element", "owner": "P::Car", "kind": "requirement", "name": "r"}),
        json!({"op": "create_element", "owner": "P::Car", "kind": "part", "name": "s"}),
        // A new definition named like the type of two usages below it.
        json!({"op": "create_element", "owner": "P::Car", "kind": "part def", "name": "Engine"}),
        // A rename onto that name, with both tools.
        json!({"op": "update_element", "ref": "P::Car::x", "rename": "s"}),
        json!({"op": "rename", "target": "P::Car::Inner", "new_name": "Engine"}),
        // A short name is a name too.
        json!({"op": "update_element", "ref": "P::Car::x", "short_name": "s"}),
        json!({"op": "create_element", "owner": "P::Car", "kind": "part", "name": "y", "short_name": "s"}),
        // An alias brings the name in for another element.
        json!({"op": "create_element", "owner": "P::Car", "kind": "alias", "name": "Engine", "target": "P::Q::Engine2"}),
        // Raw text does it as well.
        json!({"op": "add_element", "owner": "P::Car", "text": "requirement r;"}),
    ] {
        let msg = refused(&f, op.clone());
        assert!(msg.starts_with("refused: this edit would change what") && msg.contains("Fix:"), "{op}: {msg}");
        assert!(msg.contains("named P::") && msg.contains("would name P::Car::") || msg.contains("would name P::Q::Engine2"), "{op}: {msg}");
        // The same edit inside a batch is refused too, and nothing is written.
        let batch = intent::apply_batch(&f, &[json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Free"}), op.clone()]);
        let (at, fail) = batch.err().unwrap_or_else(|| panic!("the batch with {op} should fail"));
        assert_eq!(at, 1, "{op}");
        assert!(fail.message.contains("would change what"), "{op}: {}", fail.message);
    }
    assert_eq!(targets(&f), before);
    // A move brings the name of the moved element into its new owner.
    let g = files(&CAP.replace("part def Engine2;", "part def Engine;"));
    let msg = refused(&g, json!({"op": "update_element", "ref": "P::Q::Engine", "owner": "P::Car"}));
    assert!(msg.contains("named P::Engine and would name P::Car::Engine"), "{msg}");
    // Moving the element the references name keeps them on it: no capture.
    let mut h = files(CAP);
    apply(&mut h, json!({"op": "update_element", "ref": "P::Engine", "owner": "P::Car"}));
    assert!(targets(&h).contains(&("P::Car::engine".into(), "Engine".into(), Some("P::Car::Engine".into()))));
}

#[test]
fn r16b_f2_a_wave_of_creates_that_captures_a_reference_is_refused() {
    let f = files(CAP);
    // Enough creates for the batch to run as a wave; one of them takes the name `r` inside Car.
    let mut edits: Vec<Value> = (0..8).map(|i| json!({"op": "create_element", "owner": "P::Q", "kind": "part def", "name": format!("N{i}")})).collect();
    edits.insert(4, json!({"op": "create_element", "owner": "P::Car", "kind": "requirement", "name": "r"}));
    let (at, fail) = intent::apply_batch(&f, &edits).err().expect("the batch must fail");
    assert_eq!(at, 4);
    assert!(fail.message.contains("'r' in P::Car::$3 named P::r and would name P::Car::r"), "{}", fail.message);
    // Without that edit the same batch goes through.
    edits.remove(4);
    assert!(intent::apply_batch(&f, &edits).is_ok());
}

#[test]
fn r16b_f2_removing_the_name_that_hid_another_is_refused_too() {
    // Inside Car, `r` names Car's own requirement. Deleting or renaming it would hand the satisfy to the outer `r`.
    let text = "package P {\n    requirement r;\n    part s;\n    part def Car {\n        requirement r;\n        part t;\n        satisfy r by t;\n    }\n}\n";
    let f = files(text);
    for op in [json!({"op": "delete", "target": "P::Car::r"}), json!({"op": "replace", "target": "P::Car::r", "text": "requirement q;"})] {
        let msg = refused(&f, op.clone());
        assert!(msg.contains("named P::Car::r and would name P::r"), "{op}: {msg}");
    }
}

#[test]
fn r16b_f2_edits_that_capture_nothing_are_written_as_before() {
    let mut f = files(CAP);
    let before = targets(&f);
    // New names no reference uses; a name that an unresolved reference waits for; a rename that takes its references along.
    apply(&mut f, json!({"op": "create_element", "owner": "P::Car", "kind": "part", "name": "wheel", "typed_by": "P::Engine"}));
    apply(&mut f, json!({"op": "create_element", "owner": "P::Car", "kind": "requirement", "name": "r2"}));
    apply(&mut f, json!({"op": "update_element", "ref": "P::Engine", "rename": "Motor"}));
    let after = targets(&f);
    assert!(after.contains(&("P::Car::engine".into(), "Motor".into(), Some("P::Motor".into()))), "{after:?}");
    // Every reference that was there still names its element (Engine is now Motor).
    for (holder, text, target) in &before {
        let (text, target) = (text.replace("Engine", "Motor"), target.as_ref().map(|t| t.replace("P::Engine", "P::Motor")));
        assert!(after.contains(&(holder.clone(), text.clone(), target.clone())), "{holder} {text} {target:?} is gone: {after:?}");
    }
    // A feature that gets another type changes what a chain through it reaches; that is the edit asked for, not a capture.
    let mut g = files("package P {\n    part def A { attribute m = 1; }\n    part def B { attribute m = 2; }\n    part def Car {\n        part e : A;\n        attribute x = e.m;\n    }\n}\n");
    apply(&mut g, json!({"op": "update_element", "ref": "P::Car::e", "typed_by": "P::B"}));
    // A forward reference that starts to resolve is not a capture.
    let mut h = files("package P {\n    part def Car {\n        part e : Later;\n    }\n}\n");
    apply(&mut h, json!({"op": "create_element", "owner": "P", "kind": "part def", "name": "Later"}));
}

/// An import changes no name count, so the check compares every reference when the imports change. A name that a nearer
/// namespace imports wins over one declared outside it (KerML 8.2.3.5.4; round 17 made the resolver follow it), so an
/// import that brings `Engine` into `Car` would re-point the references there and is refused; nothing changes.
#[test]
fn r16b_f2_an_import_that_would_re_point_references_is_refused() {
    let f = files(&CAP.replace("part def Engine2;", "part def Engine;"));
    let before = targets(&f);
    let why = intent::apply_op(&f, &json!({"op": "create_element", "owner": "P::Car", "kind": "import", "target": "P::Q::*"})).err().expect("the import is refused").message;
    assert!(why.contains("would change what") && why.contains("P::Q::Engine"), "{why}");
    assert_eq!(before, targets(&f));
}

// ------------------------------------------------------------------ fault 3: clause text is parsed, not only scanned

const STATES: &str = "package P {\n    item def Sig;\n    action def Act;\n    attribute level = 1;\n    state def St {\n        action run;\n        state a;\n        state b;\n        state c;\n    }\n    part def Car {\n        attribute x = 1;\n    }\n    metadata def Tag { attribute lvl; }\n}\n";

fn transition(extra: Value) -> Value {
    let mut op = json!({"op": "create_relationship", "kind": "transition", "source": "P::St::a", "target": "P::St::b"});
    for (k, v) in extra.as_object().unwrap() {
        op[k] = v.clone();
    }
    op
}

/// Each of these passed the character scan and was saved. None is the clause its field names: the effect declares an
/// element inside the transition, the others are text the official grammar rejects.
#[test]
fn r16b_f3_text_that_is_not_the_clause_of_its_field_is_refused() {
    let f = files(STATES);
    for op in [
        transition(json!({"effect": "action evil : P::Act"})),
        transition(json!({"effect": "x > 1"})),
        transition(json!({"effect": "run stop"})),
        transition(json!({"effect": "run : P::Act"})),
        transition(json!({"guard": "x else c"})),
        transition(json!({"guard": "level >"})),
        transition(json!({"guard": "part q"})),
        transition(json!({"guard": "level : P::Sig"})),
        transition(json!({"trigger": "after"})),
        transition(json!({"trigger": "after 5 [s] 6"})),
        transition(json!({"trigger": "when level > "})),
        transition(json!({"trigger": "when part q"})),
        json!({"op": "create_relationship", "kind": "succession", "source": "P::St::a", "target": "P::St::b", "owner": "P::St", "guard": "a b"}),
        json!({"op": "update_element", "ref": "P::Car::x", "value": "1 2"}),
        json!({"op": "update_element", "ref": "P::Car::x", "value": "x :> y"}),
        json!({"op": "update_element", "ref": "P::Car::x", "value": "a ? b : c"}),
        json!({"op": "create_element", "owner": "P::Car", "kind": "attribute", "name": "y", "value": "part z"}),
        json!({"op": "create_element", "owner": "P::Car", "kind": "attribute", "name": "y", "value": "5 +"}),
        json!({"op": "create_element", "owner": "P::Car", "kind": "metadata", "typed_by": "P::Tag", "slots": [{"name": "lvl", "value": "1 state s"}]}),
        json!({"op": "create_element", "owner": "P::Car", "kind": "constraint", "name": "lim", "body": "x < 5 x"}),
    ] {
        let msg = refused(&f, op.clone());
        assert!(msg.contains("is not a plain expression") || msg.contains("is not the name of an action") || msg.contains("is not 'after N [unit]'") || msg.contains("is empty"), "{op}: {msg}");
    }
}

#[test]
fn r16b_f3_the_clauses_the_tools_document_are_written_and_are_valid_official_text() {
    for op in [
        transition(json!({"effect": "run"})),
        transition(json!({"effect": "send Sig() to c"})),
        transition(json!({"effect": "assign level := level + 1"})),
        transition(json!({"guard": "level > 5 and not (level == 9)"})),
        transition(json!({"trigger": "after 5 [s]"})),
        transition(json!({"trigger": "when level > 5"})),
        transition(json!({"trigger": "P::Sig"})),
        transition(json!({"trigger": "Sig", "guard": "level < 3", "effect": "run"})),
        json!({"op": "update_element", "ref": "P::Car::x", "value": "if level > 2 ? 12 [V] else level * 2"}),
        json!({"op": "create_element", "owner": "P::Car", "kind": "attribute", "name": "y", "value": "x ** 2 + P::level"}),
        json!({"op": "create_element", "owner": "P::Car", "kind": "metadata", "typed_by": "P::Tag", "slots": [{"name": "lvl", "value": "\"high\""}]}),
        json!({"op": "create_element", "owner": "P::Car", "kind": "constraint", "name": "lim", "body": "x < 5"}),
    ] {
        let mut f = files(STATES);
        // `apply` checks the saved file with the official grammar.
        apply(&mut f, op);
    }
}

/// The official recogniser reads each clause inside the statement it goes into. A build without it (wasm) has the parser
/// of the tools alone; both refuse the same probes above. Here: a form the parser of the tools reads and the grammar does not.
#[test]
fn r16b_f3_the_official_recogniser_has_the_last_word_when_it_is_there() {
    assert!(rules::official_checker().is_some(), "the tests register the recogniser");
    let f = files(STATES);
    // `xs->reduce add` names a function; a number in that place is no function reference in the official grammar.
    let msg = refused(&f, json!({"op": "update_element", "ref": "P::Car::x", "value": "x->f 1"}));
    assert!(msg.contains("is not a plain expression") || msg.contains("is not valid SysML v2"), "{msg}");
}

// ------------------------------------------------------------------ round 16 item 5: what the importer writes is read back

// ------------------------------------------------------------------ fault 5: the official files pass the syntax gate

/// The files of the official OMG release that the repository holds, and every file under `SYSML_OMG_DIR` when it is set
/// (the full release has 314 files). Each one must pass the syntax gate.
#[test]
fn r16b_every_official_file_passes_the_syntax_gate() {
    let mut paths = Vec::new();
    sysml_files_under(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../sysml-conform/tests/official"), &mut paths);
    let vendored = paths.len();
    assert!(vendored >= 42, "the repository holds {vendored} official files");
    if let Ok(dir) = std::env::var("SYSML_OMG_DIR") {
        sysml_files_under(std::path::Path::new(&dir), &mut paths);
        assert!(paths.len() > vendored, "SYSML_OMG_DIR={dir} holds no .sysml file");
    }
    paths.sort();
    let mut problems = Vec::new();
    for p in &paths {
        let text = String::from_utf8_lossy(&std::fs::read(p).unwrap()).to_string();
        let v = gate(&files(&text), "syntax");
        if v["passed"] != true {
            let f = &v["findings"][0];
            problems.push(format!("{}: [{}] line {}: {}", p.display(), f["rule"].as_str().unwrap_or(""), f["line"], f["message"].as_str().unwrap_or("")));
        }
    }
    println!("official files checked: {}; rejected: {}", paths.len(), problems.len());
    assert!(problems.is_empty(), "{} of {} official files fail the syntax gate:\n{}", problems.len(), paths.len(), problems.join("\n"));
}

#[test]
fn r16b_the_official_forms_the_gate_used_to_reject_are_read() {
    for ok in [
        "package P { part def A; connection def C { end ref source : A; end [*] ref cause : A; } }",
        "package P { part def A; individual def A1 :> A; }",
        "package P { package Q { part def A; } private import Q::**; private import Q::*::**; }",
        "package P { part def A { attribute m = 1; } part a : A { :>> m = 2 { doc /* why */ } } }",
        "package P { part def A { attribute t; attribute m; } part b : A; part a : A { :>> m = b.t { doc /* why */ } } }",
        "package P {\n    //* a block note\n       public import Q::*;\n    */\n    part def A;\n}",
    ] {
        let v = gate(&files(ok), "syntax");
        assert_eq!(v["passed"], true, "{ok}\n{v}");
        assert!(common::fault(ok).is_none(), "the official grammar rejects {ok}: {:?}", common::fault(ok));
    }
    // The forms next to them that the grammar does not allow still fail.
    for bad in ["package P { part def A; connection def C { end ref part x : A; } }", "package P { abstract def A; }", "package P { attribute x = 2 ** ; }", "package P { attribute x = 4 / ; }"] {
        assert_eq!(gate(&files(bad), "syntax")["passed"], false, "{bad}");
        assert!(common::fault(bad).is_some(), "{bad}");
    }
}

// ------------------------------------------------------------------ fault 4: the grammar scan is linear

/// The inputs of the cost report, at two sizes. The scan counts its loop turns; four times the text may take no more than
/// about four times the turns, and the turns stay within a fixed number per byte. No clock is read.
#[test]
fn r16b_the_grammar_scan_takes_work_in_step_with_the_text() {
    let make = |kind: &str, n: usize| -> String {
        match kind {
            "imports" => format!("package P {{ {}}}", "import A::*;\n".repeat(n)),
            "prefix" => format!("package P {{ {}part x; }}", "private ".repeat(n)),
            "mult" => format!("package P {{ {}}}", (0..n).map(|i| format!("part p{i} [1..];\n")).collect::<String>()),
            "open" => format!("package P {{ part x {};}}", "[ ".repeat(n)),
            "nested" => format!("package P {{ part x {}1{}; }}", "[ ".repeat(n), " ]".repeat(n)),
            "alias" => format!("package P {{ {}}}", "alias ".repeat(n)),
            "assign" => format!("package P {{ action a {{ {}}} }}", "assign x ".repeat(n)),
            "ops" => format!("package P {{ attribute x = 1{}; }}", " + 1".repeat(n)),
            "paren" => format!("package P {{ attribute x = {}1{}; }}", "( ".repeat(n), " )".repeat(n)),
            "braces" => format!("package P {{ {}{} }}", "part def A { ".repeat(n), "}".repeat(n)),
            _ => unreachable!(),
        }
    };
    for kind in ["imports", "prefix", "mult", "open", "nested", "alias", "assign", "ops", "paren", "braces"] {
        let (small, large) = (make(kind, 2_000), make(kind, 8_000));
        let (a, b) = (grammar::work_steps(&small), grammar::work_steps(&large));
        assert!(b <= a * 5, "{kind}: {a} steps for {} bytes, {b} steps for {} bytes: more than linear", small.len(), large.len());
        assert!(b <= 40 * large.len() as u64, "{kind}: {b} steps for {} bytes", large.len());
    }
}

/// `check --gates syntax` on 20 000 bare imports took 80 s, and the scan above was not the cause: each name that did not
/// resolve walked every import of its namespace, and then every member of it again to look for a library import. The
/// resolver counts the members and imports it reads; four times the text may take about four times the count.
#[test]
fn r16b_resolving_many_imports_in_one_namespace_takes_work_in_step_with_the_text() {
    let make = |kind: &str, n: usize| -> String {
        match kind {
            // The reported input: no `A` anywhere.
            "bare" => format!("package P {{ {}}}", "import A::*;\n".repeat(n)),
            // The same with the package there, so every import resolves.
            "found" => format!("package A {{ part def X; }}\npackage P {{ {}}}", "private import A::*;\n".repeat(n)),
            // Many names that resolve nowhere, beside many imports.
            "names" => format!("package A {{ part def X; }}\npackage P {{ {}{}}}", "private import A::*;\n".repeat(n / 2), (0..n / 2).map(|i| format!("part p{i} : Missing{i};\n")).collect::<String>()),
            _ => unreachable!(),
        }
    };
    for kind in ["bare", "found", "names"] {
        let steps = |n: usize| {
            sysml_core::resolve::take_work_steps();
            let m = model(&files(&make(kind, n)));
            assert!(m.elements.len() >= n);
            sysml_core::resolve::take_work_steps()
        };
        let (a, b) = (steps(1_000), steps(4_000));
        assert!(b <= a * 5 + 1_000, "{kind}: {a} steps for 1000 statements, {b} for 4000: more than linear");
    }
}

#[test]
fn r16b_the_grammar_scan_still_finds_each_fault_on_its_line() {
    let text = "package P {\n    import A::*;\n    part p [1..];\n    part q [1][2];\n    alias B;\n    action a { assign x = 1; }\n    private private part r;\n    attribute v = [1, 2];\n}\n";
    let v = gate(&files(text), "syntax");
    let got: Vec<(String, u64)> = v["findings"].as_array().unwrap().iter().filter(|f| f["rule"] != "syntax").map(|f| (f["rule"].as_str().unwrap().to_string(), f["line"].as_u64().unwrap())).collect();
    for want in [("syntax-import-visibility", 2), ("syntax-multiplicity", 3), ("syntax-multiplicity", 4), ("syntax-alias-target", 5), ("syntax-assign-operator", 6), ("syntax-repeated-modifier", 7), ("syntax-multiplicity", 8)] {
        assert!(got.contains(&(want.0.to_string(), want.1)), "missing {want:?} in {got:?}");
    }
}

fn apply(f: &mut Vec<FileIn>, op: Value) -> Value {
    let a = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("op {op} failed: {}", e.message));
    *f = a.files;
    common::assert_official(f, &op.to_string());
    a.info
}

fn refused(f: &[FileIn], op: Value) -> String {
    match intent::apply_op(f, &op) {
        Ok(a) => panic!("op {op} should have failed; it wrote:\n{}", a.files[0].text),
        Err(e) => e.message,
    }
}

#[allow(dead_code)]
fn unused() -> Value {
    json!(null)
}
