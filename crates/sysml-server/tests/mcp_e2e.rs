//! Runs the real server binary and talks to it over HTTP, as an MCP client would.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static N: AtomicUsize = AtomicUsize::new(0);

struct Server {
    child: Child,
    port: u16,
    dir: std::path::PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(files: &[(&str, String)]) -> Server {
    let dir = std::env::temp_dir().join(format!("sysml-e2e-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let child = Command::new(env!("CARGO_BIN_EXE_sysml-bridge-engine"))
        .args(["--workspace", dir.to_str().unwrap(), "--bind", "127.0.0.1", "--port", &port.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("cannot start the server");
    let s = Server { child, port, dir };
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return s;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server did not start");
}

fn post(port: u16, body: &Value) -> Value {
    let body = body.to_string();
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut raw = String::new();
    s.read_to_string(&mut raw).unwrap();
    let (head, payload) = raw.split_once("\r\n\r\n").expect("no body");
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    // The body may be chunk-encoded; take the JSON between the first '{' and the last '}'.
    let (a, b) = (payload.find('{').unwrap(), payload.rfind('}').unwrap());
    serde_json::from_str(&payload[a..=b]).unwrap()
}

fn call(s: &Server, name: &str, args: Value) -> Value {
    let r = post(s.port, &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } }));
    r["result"].clone()
}

#[test]
fn a_client_can_build_check_and_undo() {
    let s = start(&[("m.sysml", "package P {\n    part def A;\n    requirement r;\n    verification def V;\n}\n".into())]);
    let init = post(s.port, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }));
    assert_eq!(init["result"]["serverInfo"]["name"], "sysml-bridge-engine");
    let tools = post(s.port, &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
    assert!(tools["result"]["tools"].as_array().unwrap().len() >= 30);

    let state = call(&s, "get_project_state", json!({}));
    assert_eq!(state["isError"], false);
    assert_eq!(state["structuredContent"]["counts_by_kind"]["part def"], 1);

    let made = call(&s, "create_element", json!({ "owner": "P", "kind": "part", "name": "a1", "typed_by": "P::A" }));
    assert_eq!(made["structuredContent"]["applied"], true);
    let rel = call(&s, "create_relationship", json!({ "kind": "satisfy", "source": "P::a1", "target": "P::r" }));
    assert_eq!(rel["isError"], false, "{rel}");
    call(&s, "create_relationship", json!({ "kind": "verify", "source": "P::V", "target": "P::r" }));
    let gate = call(&s, "validate_model", json!({ "gate": "traceability" }));
    assert_eq!(gate["structuredContent"]["passed"], true);
    assert_eq!(call(&s, "coverage_report", json!({}))["structuredContent"]["totals"]["percent_covered"], 100.0);

    // Three accepted writes in the file on disk, and the file store does not list the log.
    let log = std::fs::read_to_string(s.dir.join(".sysml-bridge/history.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 3);
    let files = call(&s, "list_files", json!({}));
    assert_eq!(files["structuredContent"]["count"], 1);

    for _ in 0..3 {
        assert_eq!(call(&s, "undo", json!({}))["isError"], false);
    }
    let text = std::fs::read_to_string(s.dir.join("m.sysml")).unwrap();
    assert_eq!(text, "package P {\n    part def A;\n    requirement r;\n    verification def V;\n}\n");
    let bad = call(&s, "create_element", json!({ "owner": "P::Missing", "kind": "part", "name": "x" }));
    assert_eq!(bad["isError"], true);
    assert!(bad["structuredContent"]["error"].is_string());
}

/// `sysml-bridge-engine check`: one report for every gate; exit 0 only when no gate has an error.
#[test]
fn the_check_command_exits_nonzero_on_an_error_and_zero_without_one() {
    let dir = std::env::temp_dir().join(format!("sysml-check-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    std::fs::create_dir_all(&dir).unwrap();
    let run = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_sysml-bridge-engine")).arg("check").args(args).output().unwrap();
    let d = dir.to_str().unwrap();

    std::fs::write(dir.join("m.sysml"), "package P { requirement r; part def A; }\n").unwrap();
    let bad = run(&["--workspace", d]);
    assert_eq!(bad.status.code(), Some(1), "{}", String::from_utf8_lossy(&bad.stdout));
    let text = String::from_utf8_lossy(&bad.stdout);
    assert!(text.starts_with("FAILED"), "{text}");
    for g in ["syntax", "semantic", "traceability", "connections", "coverage", "official", "spec"] {
        assert!(text.contains(g), "{g} in {text}");
    }
    let v: Value = serde_json::from_slice(&run(&["--workspace", d, "--json"]).stdout).unwrap();
    assert_eq!(v["passed"], false);
    assert_eq!(v["gates"].as_object().unwrap().len(), 7);

    // Only the gates named: a model that parses and resolves passes syntax and semantic.
    let ok = run(&["--workspace", d, "--gates", "syntax,semantic"]);
    assert_eq!(ok.status.code(), Some(0), "{}", String::from_utf8_lossy(&ok.stdout));

    // A parse error fails the syntax gate.
    std::fs::write(dir.join("m.sysml"), "package P { part def A {\n").unwrap();
    assert_eq!(run(&["--workspace", d, "--gates", "syntax"]).status.code(), Some(1));

    // The official gate reads the files against the OMG grammar: a form core accepts and the grammar rejects fails it.
    std::fs::write(dir.join("m.sysml"), "package P {\n    part def A;\n    part x : A [1..];\n}\n").unwrap();
    let off = run(&["--workspace", d, "--gates", "official"]);
    assert_eq!(off.status.code(), Some(1), "{}", String::from_utf8_lossy(&off.stdout));
    let off_text = String::from_utf8_lossy(&off.stdout);
    assert!(off_text.contains("m.sysml:3:20") && off_text.contains("official"), "{off_text}");
    std::fs::write(dir.join("m.sysml"), "package P {\n    part def A;\n    part x : A;\n}\n").unwrap();
    assert_eq!(run(&["--workspace", d, "--gates", "official"]).status.code(), Some(0));

    // A bad argument is exit 2, not 0.
    assert_eq!(run(&["--workspace", d, "--gates", "nope"]).status.code(), Some(2));
    assert_eq!(run(&[]).status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}
