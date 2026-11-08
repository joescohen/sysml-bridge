//! Tools round 5: every statement an import contains can be written, read back and compared through the intent tools.

mod common;

use serde_json::{json, Value};
use sysml_core::agent::{self, intent};
use sysml_core::api::{self, FileIn};
use sysml_core::{Model, Severity};

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    api::to_model(f.to_vec())
}

fn errors(m: &Model) -> Vec<String> {
    m.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| d.message.clone()).collect()
}

/// Apply an op; the model must still parse without errors.
fn run(f: &mut Vec<FileIn>, op: Value) -> Value {
    let applied = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("op {op} failed: {}", e.message));
    *f = applied.files;
    assert!(errors(&model(f)).is_empty(), "errors after {op}: {:?}\n{}", errors(&model(f)), f[0].text);
    applied.info
}

fn fails(f: &[FileIn], op: Value) -> String {
    match intent::apply_op(f, &op) {
        Ok(_) => panic!("op {op} should have failed"),
        Err(e) => e.message,
    }
}

fn facts(f: &[FileIn]) -> Value {
    agent::export_facts(&model(f), None).unwrap()
}

fn el<'a>(v: &'a Value, r: &str) -> &'a Value {
    v["elements"].as_array().unwrap().iter().find(|e| e["ref"] == r).unwrap_or_else(|| panic!("no element {r} in {}", v["elements"].as_array().unwrap().iter().map(|e| e["ref"].as_str().unwrap_or("")).collect::<Vec<_>>().join(", ")))
}

// ---- item 1: every kind an import contains can be created ---------------------------------------------------------

/// The element kinds that export_facts shows for the converted reference workspaces (ten projects), with the call
/// that writes each. `tool` is the intent tool; the call is complete for a fixture that has the owner `S`.
fn kind_calls() -> Vec<(&'static str, Value)> {
    let ce = |v: Value| {
        let mut v = v;
        v["op"] = json!("create_element");
        v
    };
    let cr = |v: Value| {
        let mut v = v;
        v["op"] = json!("create_relationship");
        v
    };
    vec![
        ("package", ce(json!({"kind": "package", "name": "Pk", "owner": "S"}))),
        ("metadata def", ce(json!({"kind": "metadata def", "name": "Md", "owner": "S"}))),
        ("requirement", ce(json!({"kind": "requirement", "name": "r1", "owner": "S"}))),
        ("part def", ce(json!({"kind": "part def", "name": "PD", "owner": "S"}))),
        ("metadata", ce(json!({"kind": "metadata", "typed_by": "S::Md0", "owner": "S::Car"}))),
        ("part", ce(json!({"kind": "part", "name": "pp3", "owner": "S::Car"}))),
        ("attribute", ce(json!({"kind": "attribute", "name": "at", "owner": "S::Car"}))),
        ("unnamed attribute", ce(json!({"kind": "attribute", "unnamed": true, "owner": "S::Car"}))),
        ("connection def", ce(json!({"kind": "connection def", "name": "CD", "owner": "S"}))),
        ("alias", ce(json!({"kind": "alias", "name": "al", "target": "S::Car", "owner": "S"}))),
        ("constraint", ce(json!({"kind": "constraint", "name": "co", "owner": "S"}))),
        ("constraint def", ce(json!({"kind": "constraint def", "name": "CoD", "owner": "S"}))),
        ("port", ce(json!({"kind": "port", "name": "po3", "owner": "S::Car"}))),
        ("subject", ce(json!({"kind": "subject", "name": "su", "typed_by": "S::Car", "owner": "S::UCf"}))),
        ("action def", ce(json!({"kind": "action def", "name": "AD", "owner": "S"}))),
        ("action", ce(json!({"kind": "action", "name": "ac", "owner": "S::ADf"}))),
        ("parameter", ce(json!({"kind": "parameter", "name": "pa", "direction": "in", "typed_by": "ScalarValues::Real", "owner": "S::ADf"}))),
        ("fork node", ce(json!({"kind": "fork node", "name": "fk", "owner": "S::ADf"}))),
        ("join node", ce(json!({"kind": "join node", "name": "jn", "owner": "S::ADf"}))),
        ("decide node", ce(json!({"kind": "decide node", "name": "dc", "owner": "S::ADf"}))),
        ("merge node", ce(json!({"kind": "merge node", "name": "mg", "owner": "S::ADf"}))),
        ("succession", ce(json!({"kind": "succession", "source": "start", "target": "S::ADf::ac0", "owner": "S::ADf"}))),
        ("flow", cr(json!({"kind": "flow", "source": "S::Car::po", "target": "S::Car::po2", "owner": "S::Car"}))),
        ("allocation", cr(json!({"kind": "allocate", "source": "S::Car::pp", "target": "S::Car::pp2", "owner": "S::Car"}))),
        ("include use case", ce(json!({"kind": "include use case", "typed_by": "S::UCf", "owner": "S::UCf"}))),
        ("dependency", cr(json!({"kind": "dependency", "source": "S::Car", "target": "S::Car2"}))),
        ("comment", ce(json!({"kind": "comment", "body": "text", "about": "S::Car", "owner": "S"}))),
        ("empty comment", ce(json!({"kind": "comment", "body": "", "about": "S::Car", "owner": "S"}))),
        ("connection", cr(json!({"kind": "connect", "source": "S::Car::pp", "target": "S::Car::pp2", "owner": "S::Car"}))),
        ("port def", ce(json!({"kind": "port def", "name": "PoD", "owner": "S"}))),
        ("item", ce(json!({"kind": "item", "name": "it", "owner": "S::Car"}))),
        ("satisfy", cr(json!({"kind": "satisfy", "source": "S::Car::pp", "target": "S::r0"}))),
        ("item def", ce(json!({"kind": "item def", "name": "ID", "owner": "S"}))),
        ("attribute def", ce(json!({"kind": "attribute def", "name": "AtD", "owner": "S"}))),
        ("enum def", ce(json!({"kind": "enum def", "name": "ED", "owner": "S"}))),
        ("enum", ce(json!({"kind": "enum", "name": "ev", "owner": "S::EDf"}))),
        ("use case def", ce(json!({"kind": "use case def", "name": "UCD", "owner": "S"}))),
        ("use case", ce(json!({"kind": "use case", "name": "ucu", "owner": "S"}))),
        ("feature", ce(json!({"kind": "feature", "unnamed": true, "redefines": "mass", "value": "5", "owner": "S::Car2"}))),
        ("state def", ce(json!({"kind": "state def", "name": "SD", "owner": "S"}))),
        ("state", ce(json!({"kind": "state", "name": "s1", "owner": "S::SDf"}))),
        ("do action", ce(json!({"kind": "do action", "name": "da", "owner": "S::SDf"}))),
        ("entry action", ce(json!({"kind": "entry action", "owner": "S::SDf"}))),
        ("exit action", ce(json!({"kind": "exit action", "owner": "S::SDf"}))),
        ("transition", cr(json!({"kind": "transition", "source": "start", "target": "S::SDf::s0", "owner": "S::SDf"}))),
        ("occurrence", ce(json!({"kind": "occurrence", "name": "oc", "owner": "S"}))),
        ("unnamed occurrence", ce(json!({"kind": "occurrence", "unnamed": true, "doc": "gate", "owner": "S"}))),
        ("binding", ce(json!({"kind": "bind", "source": "S::Car::pp", "target": "S::Car::pp2", "owner": "S::Car"}))),
        ("actor", ce(json!({"kind": "actor", "typed_by": "S::Car", "owner": "S::UCf"}))),
        ("occurrence def", ce(json!({"kind": "occurrence def", "name": "OD", "owner": "S"}))),
        ("message", ce(json!({"kind": "message", "name": "mm", "source": "S::ODf::o1", "target": "S::ODf::o2", "owner": "S::ODf"}))),
    ]
}

/// The kinds export_facts shows for the converted workspaces. The list is the kind names only, no project content.
const IMPORT_KINDS: &[&str] = &[
    "package", "metadata def", "requirement", "part def", "metadata", "part", "attribute", "connection def", "alias", "constraint",
    "constraint def", "port", "subject", "action def", "action", "parameter", "fork node", "join node", "succession", "flow",
    "allocation", "include use case", "dependency", "comment", "connection", "port def", "item", "satisfy", "item def",
    "attribute def", "enum def", "enum", "use case def", "feature", "state def", "state", "do action", "entry action",
    "exit action", "transition", "occurrence", "decide node", "binding", "use case", "actor", "occurrence def", "message",
    "merge node",
];

const KIND_FIXTURE: &str = "package S {
    metadata def Md0;
    part def Car { attribute mass : ScalarValues::Real; part pp; part pp2; port po; port po2; }
    part def Car2 :> Car;
    enum def EDf;
    use case def UCf;
    requirement r0;
    action def ADf { action ac0; }
    occurrence def ODf { occurrence o1; occurrence o2; }
    state def SDf { state s0; }
}
";

#[test]
fn r5_create_element_accepts_every_kind_an_import_contains() {
    let calls = kind_calls();
    for k in IMPORT_KINDS {
        assert!(calls.iter().any(|(n, _)| n == k), "no call for the import kind '{k}'");
    }
    for (name, call) in &calls {
        let mut f = files(KIND_FIXTURE);
        let info = run(&mut f, call.clone());
        // What was written reads back as the kind that was asked for.
        let m = model(&f);
        let made = info["ref"].as_str().unwrap_or_else(|| panic!("{name}: no ref in {info}"));
        let e = m.find(made).unwrap_or_else(|| panic!("{name}: {made} not found\n{}", f[0].text));
        let want = IMPORT_KINDS.iter().find(|k| **k == *name).copied().or(match *name {
            "unnamed attribute" => Some("attribute"),
            "empty comment" => Some("comment"),
            "unnamed occurrence" => Some("occurrence"),
            _ => None,
        });
        if let Some(w) = want {
            assert_eq!(e.keyword(), w, "{name}: {}", f[0].text);
        }
    }
}

#[test]
fn r5_unnamed_needs_the_flag_and_other_new_kinds_have_guards() {
    let f = files(KIND_FIXTURE);
    let e = fails(&f, json!({"op": "create_element", "owner": "S::Car", "kind": "attribute"}));
    assert!(e.starts_with("missing argument 'name': the element name") && e.contains("unnamed: true"), "{e}");
    assert!(fails(&f, json!({"op": "create_element", "owner": "S", "kind": "part def", "unnamed": true})).contains("needs a name"));
    assert!(fails(&f, json!({"op": "create_element", "owner": "S", "kind": "include use case def"})).contains("no definition form"));
    let mut g = files(KIND_FIXTURE);
    run(&mut g, json!({"op": "create_element", "owner": "S::Car", "kind": "attribute", "unnamed": true}));
    assert!(sysml_core::agent::mint::strip_ids(&g[0].text).contains("attribute;"), "{}", g[0].text);
    run(&mut g, json!({"op": "create_element", "owner": "S", "kind": "comment", "body": "", "about": "S::Car"}));
    assert!(sysml_core::agent::mint::strip_ids(&g[0].text).contains("comment about S::Car /*"), "{}", g[0].text);
}

// ---- item 2: metadata prefixes -----------------------------------------------------------------------------------

#[test]
fn r5_metadata_prefixes_on_elements_and_statements() {
    let mut f = files("package S {\n    package SysMLv1 { metadata def Actor; metadata def Usage; metadata def Verify; }\n    part def Car;\n    part def Bus;\n    requirement r0;\n}\n");
    run(&mut f, json!({"op": "create_element", "owner": "S", "kind": "part def", "name": "Person", "metadata": ["SysMLv1::Actor"]}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("#SysMLv1::Actor part def Person;"), "{}", f[0].text);
    run(&mut f, json!({"op": "create_relationship", "kind": "dependency", "source": "S::Car", "target": "S::Bus", "metadata": ["SysMLv1::Usage"]}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("#SysMLv1::Usage dependency from S::Car to S::Bus;") || sysml_core::agent::mint::strip_ids(&f[0].text).contains("#S::SysMLv1::Usage dependency from"), "{}", f[0].text);
    let v = facts(&f);
    assert!(el(&v, "S::Person")["prefixes"].as_array().unwrap().iter().any(|p| p.as_str().unwrap().starts_with("#") && p.as_str().unwrap().ends_with("SysMLv1::Actor")), "{v}");
    // The same call twice is a duplicate; another tag is another statement.
    let again = fails(&f, json!({"op": "create_relationship", "kind": "dependency", "source": "S::Car", "target": "S::Bus", "metadata": ["SysMLv1::Usage"]}));
    assert!(again.contains("already exists"), "{again}");
    run(&mut f, json!({"op": "create_relationship", "kind": "dependency", "source": "S::Car", "target": "S::Bus", "metadata": ["SysMLv1::Verify"]}));
    let v = facts(&f);
    assert!(v["edges"].as_array().unwrap().iter().any(|e| e["kind"] == "verify" && e["source"] == "S::Car" && e["target"] == "S::Bus"), "a Verify tag reads back as a verify edge");
    // update_element replaces and clears the prefixes, on a named element and on an unnamed statement.
    run(&mut f, json!({"op": "update_element", "ref": "S::Person", "metadata": ["SysMLv1::Usage"]}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("#SysMLv1::Usage part def Person;") && !sysml_core::agent::mint::strip_ids(&f[0].text).contains("Actor part def"), "{}", f[0].text);
    run(&mut f, json!({"op": "update_element", "ref": "S::Person", "metadata": []}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("    part def Person;"), "{}", f[0].text);
    let dep = facts(&f)["elements"].as_array().unwrap().iter().find(|e| e["kind"] == "dependency" && e["prefixes"].is_array()).unwrap()["ref"].as_str().unwrap().to_string();
    run(&mut f, json!({"op": "update_element", "ref": dep, "metadata": []}));
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "source": "S::Car", "target": "S::r0", "metadata": ["X::Y"]})).contains("does not apply"));
}

// ---- items 3 and 4: export_facts completeness and the round trip ---------------------------------------------------

const SYNTH: &str = "package SysMLv1 {
    metadata def Actor;
    metadata def ItemFlow;
    metadata def Verify;
}
package P {
    private import SI::*;
    part def Driver;
    #SysMLv1::Actor part def Person;
    port def Link {
        in attribute amount : ScalarValues::Real;
    }
    part def Engine {
        port p : Link;
    }
    part def Tank {
        port q : ~Link;
    }
    part def Car {
        attribute mass : ScalarValues::Real;
        attribute;
        part engine : Engine;
        part spare : Engine;
        ref part tank ::> engine;
        part e2 :> engine;
        part t : Tank;
        connect engine.p to t.q;
        bind engine = spare;
    }
    part def Truck :> Car {
        :>> mass = 5;
    }
    action def Drive {
        action a1;
        action a2;
        first start then a1;
        first a1 then a2;
        first a2 then done;
        occurrence {
            doc /* gate */
        }
    }
    state def Machine {
        state Idle;
        state Run;
        entry;
        transition first start then Idle;
        transition first Idle accept after 5 [s] if \"go\" do act1 then Run;
        transition first Run then done;
        alias fin for done;
    }
    use case def Use {
        subject s : Car;
        actor d : Driver;
        actor : Driver;
        include use case : Sub;
    }
    use case def Sub;
    occurrence def Talk {
        occurrence o1;
        occurrence o2;
        message m1 from o1 to o2 {
            doc /* hi */
        }
        message from o1 to o2;
    }
    comment about Car /* hello */
    comment c0 about Car, Driver /*  */
    #SysMLv1::ItemFlow dependency 'flow for Fuel' from Engine::p to Tank::q {
        comment /* realized by Car */
    }
    #SysMLv1::Verify dependency from Car to Driver;
    allocate Drive to Car;
    allocate Drive to P;
    flow of Link from Engine::p to Tank::q;
}
";

/// Rebuild a model only from export_facts, through intent tools. This is the script an agent runs.
fn rebuild(v: &Value) -> Vec<Value> {
    let mut edits: Vec<Value> = Vec::new();
    let strs = |x: &Value| x.as_array().map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect::<Vec<_>>()).unwrap_or_default();
    for e in v["elements"].as_array().unwrap() {
        let kind = e["kind"].as_str().unwrap();
        let named = e["unnamed"] != true;
        let mut a = json!({});
        if let Some(o) = e["owner"].as_str() {
            a["owner"] = json!(o);
        }
        if named {
            a["name"] = e["name"].clone();
        }
        let prefixes = strs(&e["prefixes"]);
        let meta: Vec<String> = prefixes.iter().filter(|p| p.starts_with('#')).map(|p| p[1..].to_string()).collect();
        let mods: Vec<String> = prefixes.iter().filter(|p| !p.starts_with('#')).cloned().collect();
        let ends = |a: &mut Value| {
            a["source"] = e["ends"]["source"]["ref"].clone();
            a["target"] = e["ends"]["target"]["ref"].clone();
        };
        match kind {
            "succession" => {
                a["op"] = json!("create_relationship");
                a["kind"] = json!("succession");
                ends(&mut a);
                if let Some(g) = e.get("guard") {
                    a["guard"] = g.clone();
                }
                a["allow_duplicate"] = json!(true);
            }
            "transition" => {
                a["op"] = json!("create_relationship");
                a["kind"] = json!("transition");
                ends(&mut a);
                for k in ["trigger", "guard", "effect"] {
                    if let Some(g) = e.get(k) {
                        a[k] = g.clone();
                    }
                }
                a["allow_duplicate"] = json!(true);
            }
            "connection" if e.get("ends").is_some() => {
                a["op"] = json!("create_relationship");
                a["kind"] = json!("connect");
                ends(&mut a);
                if let Some(t) = e.get("typed_by") {
                    a["typed_by"] = t[0].clone();
                }
                a["allow_duplicate"] = json!(true);
            }
            "binding" => {
                a["op"] = json!("create_element");
                a["kind"] = json!("bind");
                ends(&mut a);
            }
            "flow" => {
                a["op"] = json!("create_relationship");
                a["kind"] = json!("flow");
                ends(&mut a);
                if let Some(t) = e.get("of") {
                    a["of"] = t[0].clone();
                }
                a["allow_duplicate"] = json!(true);
            }
            "dependency" => {
                a["op"] = json!("create_relationship");
                a["kind"] = json!("dependency");
                ends(&mut a);
                if !meta.is_empty() {
                    a["metadata"] = json!(meta);
                }
                a["allow_duplicate"] = json!(true);
            }
            "allocation" if e.get("ends").is_some() => {
                a["op"] = json!("create_relationship");
                a["kind"] = json!("allocate");
                ends(&mut a);
                a["allow_definitions"] = json!(true);
                a["allow_duplicate"] = json!(true);
            }
            "import" => {
                a["op"] = json!("create_element");
                a["kind"] = json!("import");
                a.as_object_mut().unwrap().remove("name");
                a["target"] = e["target"].clone();
                if let Some(v) = mods.iter().find(|m| matches!(m.as_str(), "private" | "public" | "protected")) {
                    a["visibility"] = json!(v);
                }
            }
            "alias" => {
                a["op"] = json!("create_element");
                a["kind"] = json!("alias");
                a["target"] = e["target"].clone();
            }
            "comment" => {
                a["op"] = json!("create_element");
                a["kind"] = json!("comment");
                a["body"] = e["doc"].clone();
                if let Some(ab) = e.get("about") {
                    a["about"] = ab.clone();
                }
            }
            "message" => {
                a["op"] = json!("create_element");
                a["kind"] = json!("message");
                ends(&mut a);
                if let Some(d) = e.get("doc") {
                    a["doc"] = d.clone();
                }
            }
            _ => {
                a["op"] = json!("create_element");
                a["kind"] = json!(kind);
                if e["unnamed"] == true && !named && !matches!(kind, "metadata" | "subject" | "entry action" | "do action" | "exit action" | "actor" | "include use case") {
                    a["unnamed"] = json!(true);
                }
                if e["unnamed"] == true && e["name"].is_string() {
                    // A redefinition slot: its name comes from the feature it redefines.
                    a.as_object_mut().unwrap().remove("name");
                    a["kind"] = json!("feature");
                    a["unnamed"] = json!(true);
                }
                for (src, dst) in [("typed_by", "typed_by"), ("specializes", "specializes"), ("redefines", "redefines"), ("references", "references")] {
                    if let Some(t) = e.get(src) {
                        a[dst] = t.clone();
                    }
                }
                for k in ["multiplicity", "value", "direction", "short_name"] {
                    if let Some(t) = e.get(k) {
                        a[k] = t.clone();
                    }
                }
                if let Some(d) = e.get("doc") {
                    a["doc"] = d.clone();
                }
                let mods: Vec<String> = mods.iter().filter(|m| matches!(m.as_str(), "abstract" | "ref" | "end" | "parallel" | "event")).cloned().collect();
                if !mods.is_empty() {
                    a["modifiers"] = json!(mods);
                }
                if !meta.is_empty() {
                    a["metadata"] = json!(meta);
                }
            }
        }
        edits.push(a);
    }
    edits
}

/// The facts without file and line, for comparing two models.
fn plain(v: &Value) -> (Vec<Value>, Vec<(String, String, String)>) {
    let mut els: Vec<Value> = v["elements"].as_array().unwrap().clone();
    for e in &mut els {
        let o = e.as_object_mut().unwrap();
        o.remove("file");
        o.remove("line");
        o.remove("source_ids");
        // The rebuild mints an id for each element the original had none for: the ids are new, the elements the same.
        if o.get("short_name").and_then(Value::as_str).map(sysml_core::agent::mint::is_minted_id).unwrap_or(false) {
            o.remove("short_name");
        }
        if let Some(ends) = o.get_mut("ends") {
            for r in ["source", "target"] {
                ends[r].as_object_mut().unwrap().remove("text");
            }
        }
    }
    els.sort_by(|a, b| a["ref"].as_str().cmp(&b["ref"].as_str()));
    let mut edges: Vec<(String, String, String)> = v["edges"].as_array().unwrap().iter().map(|e| (e["kind"].as_str().unwrap().into(), e["source"].as_str().unwrap().into(), e["target"].as_str().unwrap().into())).collect();
    edges.sort();
    (els, edges)
}

#[test]
fn r5_a_model_rebuilt_only_from_export_facts_has_the_same_export_facts() {
    let original = files(SYNTH);
    assert!(errors(&model(&original)).is_empty(), "{:?}", errors(&model(&original)));
    let v = facts(&original);
    // The constructs the round trip must cover are in the facts, in the form the tools take.
    assert_eq!(el(&v, "P::Car::tank")["references"], json!(["P::Car::engine"]));
    assert_eq!(el(&v, "P::Tank::q")["typed_by"], json!(["~P::Link"]));
    assert_eq!(el(&v, "P::Car::mass")["typed_by"], json!(["ScalarValues::Real"]));
    assert_eq!(el(&v, "P::Machine::fin")["target"], "done");
    assert!(v["edges"].as_array().unwrap().iter().any(|e| e["kind"] == "reference" && e["source"] == "P::Car::tank" && e["target"] == "P::Car::engine"));
    let tr = v["elements"].as_array().unwrap().iter().find(|e| e["trigger"] == "after 5 [s]").expect("a trigger");
    assert_eq!((tr["guard"].as_str(), tr["effect"].as_str()), (Some("\"go\""), Some("act1")));
    let edits = rebuild(&v);
    assert!(edits.len() <= intent::MAX_EDITS, "{}", edits.len());
    let batch = intent::apply_batch(&[FileIn { path: "m.sysml".into(), text: String::new() }], &edits).unwrap_or_else(|(i, f)| panic!("edit #{} {} failed: {}", i + 1, edits[i], f.message));
    let rebuilt = batch.files;
    assert!(errors(&model(&rebuilt)).is_empty(), "{:?}\n{}", errors(&model(&rebuilt)), rebuilt[0].text);
    let (a, ea) = plain(&v);
    let (b, eb) = plain(&facts(&rebuilt));
    assert_eq!(a.len(), b.len(), "element count\n{}", rebuilt[0].text);
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x, y, "element differs");
    }
    assert_eq!(ea, eb, "edges differ\n{}", rebuilt[0].text);
}

// ---- item 5 and H15: start and done ------------------------------------------------------------------------------

#[test]
fn r5_start_and_done_are_ends_of_transitions_and_alias_targets_and_a_user_start_is_reported() {
    let mut f = files("package S {\n    state def M { state A; state B; }\n    action def Act { action x; item def start; }\n}\n");
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "start", "target": "S::M::A", "owner": "S::M"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "S::M::B", "target": "done"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "transition", "source": "start", "target": "done", "owner": "S::M", "trigger": "after 5 [s]"}));
    run(&mut f, json!({"op": "create_element", "kind": "alias", "name": "fin", "target": "done", "owner": "S::M"}));
    assert!(sysml_core::agent::mint::strip_ids(&f[0].text).contains("transition first start then A;") && sysml_core::agent::mint::strip_ids(&f[0].text).contains("then done;") && sysml_core::agent::mint::strip_ids(&f[0].text).contains("alias fin for done;"), "{}", f[0].text);
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "transition", "source": "done", "target": "S::M::A", "owner": "S::M"})).contains("cannot use"));
    // The user element named `start` hides the built-in one, and validate_model says so. The tools still write the marker.
    let v = agent::trace::validate_model(&model(&f), Some("semantic")).unwrap();
    let hit: Vec<&Value> = v["findings"].as_array().unwrap().iter().filter(|x| x["rule"] == "reserved-name-hides-marker").collect();
    assert_eq!(hit.len(), 1, "{v}");
    assert!(hit[0]["message"].as_str().unwrap().contains("S::Act::start"), "{v}");
    // if_absent with `start` is idempotent even when `start` resolves to that element.
    let call = json!({"op": "create_relationship", "kind": "succession", "source": "start", "target": "S::Act::x", "owner": "S::Act", "if_absent": true});
    let first = run(&mut f, call.clone());
    assert_ne!(first["existed"], true);
    let before = f[0].text.clone();
    let second = run(&mut f, call);
    assert_eq!(second["existed"], true, "{second}");
    assert_eq!(f[0].text, before);
    // The same for a transition.
    let tcall = json!({"op": "create_relationship", "kind": "transition", "source": "start", "target": "S::M::B", "owner": "S::M", "if_absent": true});
    run(&mut f, tcall.clone());
    assert_eq!(run(&mut f, tcall)["existed"], true);
}

// ---- H6: a statement allowed twice does not block unrelated adds ----------------------------------------------------

#[test]
fn r5_an_allowed_duplicate_does_not_refuse_an_unrelated_raw_add() {
    let mut f = files("package S {\n    part def Pump;\n    part def Tank;\n    part def Car { part a; part b; }\n}\n");
    run(&mut f, json!({"op": "create_relationship", "kind": "connect", "source": "S::Car::a", "target": "S::Car::b"}));
    run(&mut f, json!({"op": "create_relationship", "kind": "connect", "source": "S::Car::a", "target": "S::Car::b", "allow_duplicate": true}));
    // Inserts before the duplicate pair, so every later element id moves.
    run(&mut f, json!({"op": "add_element", "owner": "S::Pump", "text": "attribute x;"}));
    run(&mut f, json!({"op": "add_element", "owner": "S::Tank", "text": "state ZzQ;"}));
    // A new repeat is still refused, and allow_duplicate lets it through.
    let e = fails(&f, json!({"op": "add_element", "owner": "S::Car", "text": "connect a to b;"}));
    assert!(e.contains("already exists"), "{e}");
    run(&mut f, json!({"op": "add_element", "owner": "S::Car", "text": "connect a to b;", "allow_duplicate": true}));
}

// ---- H13: a flow between ends in two definitions ----------------------------------------------------------------

#[test]
fn r5_a_flow_between_two_definitions_goes_to_their_nearest_common_owner() {
    let mut f = files("package S {\n    port def L;\n    part def A { port p : L; }\n    part def B { port q : ~L; }\n}\n");
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "flow", "source": "S::A::p", "target": "S::B::q", "name": "f1"}));
    assert_eq!(info["owner_used"], "S", "{info}");
    assert!(info["owner_how"].as_str().unwrap().contains("nearest"), "{info}");
    let info = run(&mut f, json!({"op": "create_relationship", "kind": "flow", "source": "S::A::p", "target": "S::B::q", "owner": "S", "name": "f2", "allow_duplicate": true}));
    assert_eq!(info["owner_used"], "S");
    assert!(info["owner_how"].as_str().unwrap().contains("given"));
    let v = facts(&f);
    assert!(v["edges"].as_array().unwrap().iter().filter(|e| e["kind"] == "flow" && e["source"] == "S::A::p" && e["target"] == "S::B::q").count() == 2, "{v}");
    // A connector still needs a part to live in.
    assert!(fails(&f, json!({"op": "create_relationship", "kind": "connect", "source": "S::A::p", "target": "S::B::q"})).contains("package"));
}

// ---- H16 and H7: messages -----------------------------------------------------------------------------------------

#[test]
fn r5_messages_do_not_double_the_quotes_of_a_quoted_name_and_allocate_to_a_package_gets_the_right_hint() {
    let f = files("package 'My Pkg' {\n    requirement r;\n    part def D;\n    action def Act;\n    package 'Sub Pkg';\n}\n");
    let v = agent::trace::validate_model(&model(&f), Some("traceability")).unwrap();
    let all = v.to_string();
    assert!(all.contains("not satisfied"), "{all}");
    assert!(!all.contains("''"), "doubled quotes in {all}");
    let e = fails(&f, json!({"op": "create_relationship", "kind": "allocate", "source": "'My Pkg'::Act", "target": "'My Pkg'::'Sub Pkg'"}));
    assert!(!e.contains("''") && e.contains("allow_definitions: true"), "{e}");
    let e = fails(&f, json!({"op": "create_relationship", "kind": "allocate", "source": "'My Pkg'::'Sub Pkg'", "target": "'My Pkg'::Act", "allow_definitions": true}));
    assert!(e.contains("only be the target"), "{e}");
    let mut g = f.clone();
    let info = run(&mut g, json!({"op": "create_relationship", "kind": "allocate", "source": "'My Pkg'::Act", "target": "'My Pkg'::'Sub Pkg'", "allow_definitions": true}));
    assert_eq!(info["warnings"][0]["rule"], "trace-operand-definition");
    let e = fails(&f, json!({"op": "create_relationship", "kind": "satisfy", "source": "'My Pkg'::D", "target": "'My Pkg'::r"}));
    assert!(!e.contains("''"), "{e}");
}

// ---- H19: a failed batch lists every failing edit ------------------------------------------------------------------

#[test]
fn r5_a_failed_batch_lists_every_failing_edit() {
    let f = files("package S { part def A; }\n");
    let edits = vec![
        json!({"op": "create_element", "owner": "S", "kind": "part", "name": "ok1", "typed_by": "S::A"}),
        json!({"op": "create_element", "owner": "S", "kind": "part", "name": "bad1", "typed_by": "S::Nope"}),
        json!({"op": "create_element", "owner": "S", "kind": "part", "name": "ok2", "typed_by": "S::A"}),
        json!({"op": "create_element", "owner": "S", "kind": "bogus", "name": "bad2"}),
        json!({"op": "create_relationship", "kind": "flow", "source": "S::ok1", "target": "S::Missing"}),
    ];
    let (i, fail) = intent::apply_batch(&f, &edits).unwrap_err();
    assert_eq!(i, 1);
    let list = fail.data["failures"].as_array().unwrap();
    assert_eq!(list.iter().map(|x| x["edit"].as_u64().unwrap()).collect::<Vec<_>>(), vec![2, 4, 5], "{}", fail.data);
    assert!(fail.message.contains("#4") && fail.message.contains("#5"), "{}", fail.message);
    // One failure stays as before.
    let (_, one) = intent::apply_batch(&f, &edits[..2]).unwrap_err();
    assert_eq!(one.data["failures"].as_array().unwrap().len(), 1);
}
