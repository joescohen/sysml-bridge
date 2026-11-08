//! Limits stress round 2 hit: a shared bus with nested ports under a conjugated outer port, a named flow
//! final node.

use serde_json::json;
use sysml_core::agent::{intent, rules};
use sysml_core::api::{self, FileIn};
use sysml_core::model::Model;

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "p.sysml".into(), text: text.into() }]
}

fn model(f: &[FileIn]) -> Model {
    api::to_model(f.to_vec())
}

/// The error findings of the default gates, as "rule element".
fn errors(m: &Model) -> Vec<String> {
    let r = rules::check_all(m, None, None).unwrap();
    r["findings"].as_array().unwrap().iter().filter(|f| f["severity"] == "error").map(|f| format!("{} {}", f["rule"].as_str().unwrap_or(""), f["element"].as_str().unwrap_or(""))).collect()
}

/// One connection per node carries both directions: the node's port `lan : LanNodePort` holds `tx` (sends) and `rx`
/// (receives); the switch's port `p1 : ~LanNodePort` is its conjugate, so its `tx` receives and its `rx` sends.
const BUS: &str = "package Bus {
    item def LanFrame;
    port def LanFramePort { out item lanFrame : LanFrame; }
    port def LanNodePort {
        port tx : LanFramePort;
        port rx : ~LanFramePort;
    }
    part def Node { port lan : LanNodePort; }
    part def Switch {
        port p1 : ~LanNodePort;
        port p2 : ~LanNodePort;
    }
    part def Station {
        part a : Node;
        part b : Node;
        part sw : Switch;
        interface aLink connect a.lan to sw.p1;
        interface bLink connect b.lan to sw.p2;
        flow of LanFrame from a.lan.tx to sw.p1.tx;
        flow of LanFrame from sw.p1.rx to a.lan.rx;
        flow of LanFrame from b.lan.tx to sw.p2.tx;
        flow of LanFrame from sw.p2.rx to b.lan.rx;
    }
}
";

#[test]
fn a_bus_of_nested_ports_under_a_conjugated_port_carries_both_ways() {
    let m = model(&files(BUS));
    let bad = errors(&m);
    assert!(bad.is_empty(), "{bad:?}");
    // Both ends sending is still refused: a flow from the node's tx to the switch's rx.
    let wrong = BUS.replace("flow of LanFrame from a.lan.tx to sw.p1.tx;", "flow of LanFrame from a.lan.tx to sw.p1.rx;");
    let bad = errors(&model(&files(&wrong)));
    assert!(bad.iter().any(|b| b.starts_with("same-direction-ports")), "{bad:?}");
}

const FF: &str = "package F {
    part def Unit {
        action def Run {
            first start then watch;
            action watch;
        }
    }
}
";

/// `flow final tripEnd;` is no SysML v2: the flow final node is a name for `done`, which create_element writes.
#[test]
fn a_named_flow_final_node_takes_a_succession() {
    let mut f = files(FF);
    let r = intent::apply_op(&mut f, &json!({ "op": "create_element", "kind": "flow final", "owner": "F::Unit::Run", "name": "tripEnd" })).unwrap_or_else(|e| panic!("{}", e.message));
    f = r.files;
    assert!(f[0].text.contains("alias tripEnd for done;"), "{}", f[0].text);
    let r = intent::apply_op(&mut f, &json!({ "op": "create_relationship", "kind": "succession", "source": "F::Unit::Run::watch", "target": "F::Unit::Run::tripEnd" })).unwrap_or_else(|e| panic!("{}", e.message));
    f = r.files;
    assert!(f[0].text.contains("then tripEnd;"), "{}", f[0].text);
    let bad = errors(&model(&f));
    assert!(bad.is_empty(), "{bad:?}\n{}", f[0].text);
    // Only a name: a target is refused.
    let e = intent::apply_op(&mut files(FF), &json!({ "op": "create_element", "kind": "flow final", "owner": "F::Unit::Run", "name": "x", "target": "start" })).err().expect("refused");
    assert!(e.message.contains("only a name"), "{}", e.message);
}

/// A requirements sheet whose text column is "Requirement" (terra T2 run 2), or any header holding requirement, text,
/// statement or description, imports; a sheet with none names the option that picks the column.
#[test]
fn a_requirement_column_named_requirement_imports() {
    use sysml_core::agent::reqimport;
    let m = model(&files("package P { }"));
    for csv in ["ID,Level,Requirement,Derived from\nSYS-1,System,The car shall stop.,\n", "Req ID,Requirement statement\nSYS-1,The car shall stop.\n"] {
        let p = reqimport::plan(&m, &json!({ "csv": csv, "owner": "P" })).unwrap_or_else(|f| panic!("{}", f.message));
        assert!(p.edits.iter().any(|e| e.to_string().contains("The car shall stop.")), "{:?}", p.edits);
    }
    let e = reqimport::plan(&m, &json!({ "csv": "ID,Words\nSYS-1,The car shall stop.\n", "owner": "P" })).err().unwrap();
    assert!(e.message.contains("--text-column"), "{}", e.message);
}
