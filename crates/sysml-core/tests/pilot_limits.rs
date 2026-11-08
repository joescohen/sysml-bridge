//! Limits pilot builds hit: an accept's payload is an out pin, edits by a relationship's ends,
//! positional refs that hold inside one batch, the typed succession flow.

use serde_json::{json, Value};
use sysml_core::agent::intent;
use sysml_core::api::{self, FileIn};
use sysml_core::model::{Model, Severity};

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "p.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    api::to_model(f.to_vec())
}

fn clean(m: &Model, text: &str) {
    let bad: Vec<_> = m.diagnostics.iter().filter(|d| d.severity == Severity::Error || d.message.starts_with("cannot resolve")).map(|d| d.message.clone()).collect();
    assert!(bad.is_empty(), "{bad:?}\n{text}");
}

fn run(f: &mut Vec<FileIn>, op: Value) -> Value {
    let applied = intent::apply_op(f, &op).unwrap_or_else(|e| panic!("op {op} failed: {}", e.message));
    *f = applied.files;
    clean(&model(f), &f[0].text);
    applied.info
}

const ACCEPT: &str = "package R {
    item def Path;
    item def Status;
    port def PathPort { in item path : Path; }
    part def Drive {
        port path : PathPort;
        action def DriveWheels { in item path : Path; out item status : Status; }
        state def Modes {
            entry; then idle;
            state idle;
            state driving {
                do action run {
                    action getPath accept path : R::Path via R::Drive::path;
                    action drive : DriveWheels;
                    succession flow of R::Path from getPath.path to drive.path;
                }
            }
            transition go first idle accept p : R::Path via path then driving;
        }
    }
}
";

#[test]
fn an_accept_payload_is_an_out_pin_a_flow_carries_on() {
    let f = files(ACCEPT);
    let m = model(&f);
    clean(&m, ACCEPT);
    let pin = m.find("R::Drive::Modes::driving::run::getPath::path").expect("the payload is a member of the accept");
    assert_eq!(pin.direction.as_deref(), Some("out"));
    assert_eq!(pin.kind, "item");
    let ty = pin.first_resolved(sysml_core::model::RefRole::Type).map(|t| m.elements[t].qualified_name.clone());
    assert_eq!(ty.as_deref(), Some("R::Path"));
    // A transition's trigger keeps its payload local: no member.
    let go = m.find("R::Drive::Modes::go").unwrap();
    assert!(go.children.is_empty(), "{:?}", go.children);
    // The unnamed form and the typed op write the same pin.
    let mut f = f;
    run(&mut f, json!({ "op": "create_element", "owner": "R::Drive::Modes::driving::run", "kind": "accept action", "name": "again", "payload": "p2 : R::Path", "via": "R::Drive::path" }));
    let m = model(&f);
    assert!(m.find("R::Drive::Modes::driving::run::again::p2").is_some(), "{}", f[0].text);
}

const RELS: &str = "package R {
    item def Path;
    requirement def <'R-1'> Fast;
    requirement fast : Fast;
    requirement slow : Fast;
    part def Nav { attribute rate : Real; }
    part def Sys { part nav : Nav; }
    satisfy fast by Sys::nav;
    satisfy slow by Sys::nav;
    satisfy fast by Sys;
    action def Plan { out item path : Path; }
    action def Move { in item path : Path; }
    action def Drive {
        action plan : Plan;
        action move : Move;
        action again : Move;
        flow of Path from plan.path to move.path;
        succession flow of Path from plan.path to again.path;
        first plan then move;
    }
}
";

fn kinds_in(f: &[FileIn], owner: &str) -> Vec<String> {
    let m = model(f);
    let o = m.find(owner).unwrap();
    o.children.iter().map(|c| m.elements[*c].keyword()).collect()
}

#[test]
fn delete_relationship_by_its_ends() {
    let mut f = files(RELS);
    clean(&model(&f), RELS);
    let n = kinds_in(&f, "R").iter().filter(|k| k.as_str() == "satisfy").count();
    let info = run(&mut f, json!({ "op": "delete_relationship", "kind": "satisfy", "source": "R::Sys::nav", "target": "R::slow" }));
    assert_eq!(info["deleted"].as_array().unwrap().len(), 1, "{info}");
    assert!(!f[0].text.contains("satisfy slow"), "{}", f[0].text);
    assert!(f[0].text.contains("satisfy fast by Sys::nav") && f[0].text.contains("satisfy fast by Sys;"), "{}", f[0].text);
    assert_eq!(kinds_in(&f, "R").iter().filter(|k| k.as_str() == "satisfy").count(), n - 1);
    // A flow by its pins as written, and the succession flow apart from the plain flow.
    run(&mut f, json!({ "op": "delete_relationship", "kind": "succession flow", "source": "plan.path", "target": "again.path", "owner": "R::Drive" }));
    assert!(!f[0].text.contains("succession flow") && f[0].text.contains("flow of Path from plan.path to move.path"), "{}", f[0].text);
    run(&mut f, json!({ "op": "delete_relationship", "kind": "flow", "source": "R::Drive::plan::path", "target": "R::Drive::move::path", "of": "R::Path" }));
    assert!(!f[0].text.contains("flow of Path"), "{}", f[0].text);
    run(&mut f, json!({ "op": "delete_relationship", "kind": "succession", "source": "R::Drive::plan", "target": "R::Drive::move" }));
    assert!(!f[0].text.contains("first plan then move"), "{}", f[0].text);
    // Nothing that matches: refused with the reason.
    let e = intent::apply_op(&f, &json!({ "op": "delete_relationship", "kind": "satisfy", "source": "R::Sys::nav", "target": "R::slow" })).unwrap_err();
    assert!(e.message.contains("no satisfy from"), "{}", e.message);
    let e = intent::apply_op(&f, &json!({ "op": "delete_relationship", "kind": "frobnicate", "source": "R::Sys", "target": "R::fast" })).unwrap_err();
    assert!(e.message.contains("unknown relationship kind"), "{}", e.message);
}

#[test]
fn positional_refs_hold_inside_one_batch() {
    // The three satisfy statements are R::$5, R::$6, R::$7 (after the item, requirement def and two requirements, the two part defs).
    let f = files(RELS);
    let m = model(&f);
    let r = m.find("R").unwrap();
    let sats: Vec<String> = r.children.iter().map(|c| &m.elements[*c]).filter(|e| e.kind == "satisfy").map(|e| e.qualified_name.clone()).collect();
    assert_eq!(sats.len(), 3);
    // Delete the first, then the third by the ref it had before the batch, then document the second: each names its own.
    let edits = vec![
        json!({ "op": "delete_element", "ref": sats[0] }),
        json!({ "op": "delete_element", "ref": sats[2] }),
        json!({ "op": "update_element", "ref": sats[1], "doc": "kept" }),
    ];
    let b = intent::apply_batch(&f, &edits).unwrap_or_else(|e| panic!("{}", e.1.message));
    let t = &b.files[0].text;
    assert!(!t.contains("satisfy fast by Sys::nav") && !t.contains("satisfy fast by Sys;"), "{t}");
    assert!(t.contains("satisfy slow by Sys::nav") && t.contains("kept"), "{t}");
    clean(&model(&b.files), t);
}

#[test]
fn a_transition_takes_its_doc() {
    let src = "package R {
    part def Hoist {
        state def Modes {
            entry; then idle;
            state idle;
            state lifting;
        }
    }
}
";
    let mut f = files(src);
    let info = run(&mut f, json!({ "op": "create_relationship", "kind": "transition", "source": "R::Hoist::Modes::idle", "target": "R::Hoist::Modes::lifting", "doc": "Starts a lift." }));
    assert!(f[0].text.contains("then lifting {") && f[0].text.contains("doc /* Starts a lift. */"), "{}", f[0].text);
    let m = model(&f);
    let t = m.find(info["ref"].as_str().unwrap()).expect("the transition");
    assert!(m.children(t.id).any(|c| c.kind == "doc" || c.kind == "comment"), "{}", f[0].text);
    // Marked ends take it too.
    let mut g = files(src);
    run(&mut g, json!({ "op": "create_relationship", "kind": "transition", "source": "R::Hoist::Modes::lifting", "target": "done", "owner": "R::Hoist::Modes", "doc": "Ends." }));
    assert!(g[0].text.contains("then done {") && g[0].text.contains("doc /* Ends. */"), "{}", g[0].text);
    // Another kind refuses doc rather than dropping it.
    let e = intent::apply_op(&mut files(src), &json!({ "op": "create_relationship", "kind": "satisfy", "source": "R::Hoist", "target": "R::Hoist::Modes", "doc": "x" })).err().expect("refused");
    assert!(e.message.contains("'doc' does not apply"), "{}", e.message);
}
