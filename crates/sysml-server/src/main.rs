#![recursion_limit = "512"]
//! sysml-bridge-engine: stores `.sysml` files in a workspace folder and exposes the model to agents over MCP, on
//! standard input and output (`--stdio`) or as JSON-RPC on `POST /mcp`.

mod mcp;
mod official;
mod store;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use store::Store;

pub struct App {
    pub store: Store,
    pub token: Option<String>,
    /// Host names this server answers to. A name that merely resolves here is refused.
    pub hosts: Vec<String>,
}

type Shared = Arc<App>;

fn arg(name: &str, default: &str) -> String {
    let args: Vec<String> = std::env::args().collect();
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| default.to_string())
}

fn exit_with(code: i32, text: String) -> ! {
    if code == 0 {
        println!("{text}");
    } else {
        eprintln!("{text}");
    }
    std::process::exit(code);
}

#[tokio::main]
async fn main() {
    official::register();
    match std::env::args().nth(1).as_deref() {
        Some("check") => std::process::exit(check_command()),
        Some("clear-suspect") => {
            let (code, text) = clear_command(&std::env::args().skip(2).collect::<Vec<_>>());
            exit_with(code, text)
        }
        Some("ignore-result") => {
            let (code, text) = ignore_command(&std::env::args().skip(2).collect::<Vec<_>>());
            exit_with(code, text)
        }
        _ => {}
    }
    if std::env::args().any(|a| a == "--help" || a == "-h") {
        println!("sysml-bridge-engine check --workspace DIR [--gates syntax,semantic,traceability,connections,coverage,official,spec] [--json] [--limit N]\n  runs every gate and prints one report; exit code 0 only when no gate has an error (1 on errors, 2 on a bad argument).\n\nsysml-bridge-engine [--workspace DIR] [--bind ADDR] [--port N] [--allow-host NAME,NAME] [--stdio]\n\n--stdio serves MCP on standard input and output for a local agent and opens no port.\nWithout --stdio the server answers JSON-RPC on POST /mcp and GET /api/health.\nRequests are answered only for the bind address, localhost and the names in --allow-host.\nSet SYSML_BRIDGE_TOKEN to require a bearer token for /mcp.\n\n{CLEAR_USAGE}\n  a person clears a suspect link (list_suspect_links names it). No agent tool does this.\n\n{IGNORE_USAGE}\n  a person sets one rule result aside (the fingerprint check_all showed). No agent tool does this.");
        return;
    }
    let workspace = PathBuf::from(arg("--workspace", "workspace"));
    let bind = arg("--bind", "127.0.0.1");
    let port: u16 = arg("--port", "7878").parse().expect("--port must be a number");
    std::fs::create_dir_all(&workspace).expect("cannot create workspace folder");

    let mut hosts: Vec<String> = vec![bind.to_ascii_lowercase(), "localhost".into(), "127.0.0.1".into(), "::1".into()];
    hosts.extend(arg("--allow-host", "").split(',').map(|h| h.trim().to_ascii_lowercase()).filter(|h| !h.is_empty()));
    let app = Arc::new(App { store: Store::new(workspace.clone()), token: std::env::var("SYSML_BRIDGE_TOKEN").ok().filter(|t| !t.is_empty()), hosts });
    if std::env::args().any(|a| a == "--stdio") {
        // A local agent drives the workspace directly; no network listener is opened.
        tokio::task::spawn_blocking(move || mcp::stdio(&app)).await.ok();
        return;
    }
    let router = Router::new()
        .route("/api/health", get(|| async { Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") })) }))
        .route("/mcp", post(mcp::handle).get(|| async { StatusCode::METHOD_NOT_ALLOWED }))
        .layer(axum::middleware::from_fn_with_state(app.clone(), known_host))
        .with_state(app);

    let addr: SocketAddr = format!("{bind}:{port}").parse().expect("bad --bind address");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("cannot bind port");
    println!("sysml-bridge-engine on http://{addr}  workspace={}", workspace.display());
    axum::serve(listener, router).await.unwrap();
}

/// Host name of a `Host` header or an origin, without scheme, port or brackets.
fn host_name(value: &str) -> String {
    let v = value.split_once("://").map(|(_, h)| h).unwrap_or(value);
    let v = v.split('/').next().unwrap_or(v);
    let name = match v.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(rest),
        None => v.rsplit_once(':').map(|(h, _)| h).unwrap_or(v),
    };
    name.to_ascii_lowercase()
}

/// Refuse a request addressed to a name this server does not own. Without this,
/// a hostile site could point its own name at this address and be treated as same-origin.
async fn known_host(State(app): State<Shared>, req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let host = req.headers().get("host").and_then(|v| v.to_str().ok()).map(host_name).unwrap_or_default();
    if !app.hosts.iter().any(|h| *h == host) {
        return err(StatusCode::FORBIDDEN, "unknown host name; start the server with --allow-host to permit it");
    }
    next.run(req).await
}

/// A browser page from another site must not write here on the user's behalf.
/// Browsers always send `Origin` on such requests; other clients send none.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) else { return true };
    let host = headers.get("host").and_then(|v| v.to_str().ok()).unwrap_or("");
    let origin_host = origin.split_once("://").map(|(_, h)| h).unwrap_or(origin);
    !host.is_empty() && origin_host.eq_ignore_ascii_case(host)
}

pub fn authorized(app: &App, headers: &HeaderMap) -> bool {
    if !same_origin(headers) {
        return false;
    }
    match &app.token {
        None => true,
        Some(t) => headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.strip_prefix("Bearer ").unwrap_or(v) == t)
            .unwrap_or(false),
    }
}

fn err(status: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (status, Json(json!({ "error": msg.into() }))).into_response()
}

const IGNORE_USAGE: &str = "sysml-bridge-engine ignore-result --workspace DIR --fingerprint FP --by NAME [--note TEXT]";

/// `sysml-bridge-engine ignore-result ...`: a person sets a rule result aside at a terminal.
pub fn ignore_command(args: &[String]) -> (i32, String) {
    let (mut workspace, mut fp, mut by, mut note) = (None::<String>, None::<String>, None::<String>, None::<String>);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            k @ ("--workspace" | "--fingerprint" | "--by" | "--note") => {
                let Some(v) = args.get(i + 1).cloned() else { return (2, format!("{k} needs a value\nusage: {IGNORE_USAGE}")) };
                match k {
                    "--workspace" => workspace = Some(v),
                    "--fingerprint" => fp = Some(v),
                    "--by" => by = Some(v),
                    _ => note = Some(v),
                }
                i += 1;
            }
            other => return (2, format!("unknown argument '{other}'\nusage: {IGNORE_USAGE}")),
        }
        i += 1;
    }
    let (Some(dir), Some(fp), Some(by)) = (workspace, fp, by) else {
        return (2, format!("--workspace, --fingerprint and --by are required\nusage: {IGNORE_USAGE}"));
    };
    let root = PathBuf::from(&dir);
    if !root.is_dir() {
        return (2, format!("workspace '{dir}' is not a folder"));
    }
    let app = App { store: Store::new(root), token: None, hosts: vec![] };
    let mut a = json!({ "fingerprint": fp, "by": by });
    if let Some(n) = note {
        a["note"] = json!(n);
    }
    match mcp::suspect::ignore_result(&app, &a, &mcp::suspect::Reviewer { route: "command", authenticated: false }) {
        Ok(o) => (0, o.text),
        Err(e) => (1, e.message),
    }
}

const CLEAR_USAGE: &str = "sysml-bridge-engine clear-suspect --workspace DIR --link KEY --by NAME --since REVISION --fingerprint FP --fingerprint-before FP [--note TEXT] [--tracked-end requirement|other|both] [--fields FIELD,FIELD]";

/// `sysml-bridge-engine clear-suspect ...`: a person clears a suspect link at a terminal. Returns the exit code and the text to print.
pub fn clear_command(args: &[String]) -> (i32, String) {
    let (mut workspace, mut link, mut by, mut note, mut track) = (None::<String>, None::<String>, None::<String>, None::<String>, None::<String>);
    let (mut fingerprint, mut since, mut fp_before, mut fields) = (None::<String>, None::<String>, None::<String>, None::<String>);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            k @ ("--workspace" | "--link" | "--by" | "--note" | "--tracked-end" | "--fingerprint" | "--fingerprint-before" | "--since" | "--fields") => {
                let Some(v) = args.get(i + 1).cloned() else { return (2, format!("{k} needs a value\nusage: {CLEAR_USAGE}")) };
                match k {
                    "--workspace" => workspace = Some(v),
                    "--link" => link = Some(v),
                    "--by" => by = Some(v),
                    "--tracked-end" => track = Some(v),
                    "--fingerprint" => fingerprint = Some(v),
                    "--since" => since = Some(v),
                    "--fingerprint-before" => fp_before = Some(v),
                    "--fields" => fields = Some(v),
                    _ => note = Some(v),
                }
                i += 1;
            }
            other => return (2, format!("unknown argument '{other}'\nusage: {CLEAR_USAGE}")),
        }
        i += 1;
    }
    let (Some(dir), Some(link), Some(by)) = (workspace, link, by) else {
        return (2, format!("--workspace, --link and --by are required\nusage: {CLEAR_USAGE}"));
    };
    let root = PathBuf::from(&dir);
    if !root.is_dir() {
        return (2, format!("workspace '{dir}' is not a folder"));
    }
    let app = App { store: Store::new(root), token: None, hosts: vec![] };
    let mut a = json!({ "link": link, "by": by });
    if let Some(n) = note {
        a["note"] = json!(n);
    }
    if let Some(t) = track {
        a["tracked_end"] = json!(t);
    }
    // The fields the listing used, names joined by commas.
    if let Some(f) = fields {
        a["fields"] = json!(f.split(',').map(str::trim).collect::<Vec<_>>());
    }
    // Without the fingerprint nothing is cleared: the command shows what the link is about to be cleared with (the change since
    // --since, when given) and asks for the fingerprint it read.
    if let Some(sv) = &since {
        a["since"] = json!(sv);
    }
    let (Some(fp), Some(fpb)) = (fingerprint.clone(), fp_before.clone()) else {
        let mut shown = String::new();
        if let Some(sv) = since {
            let mut q = json!({ "since": sv });
            for k in ["tracked_end", "fields"] {
                if let Some(t) = a.get(k) {
                    q[k] = t.clone();
                }
            }
            if let Ok(o) = mcp::suspect::list_suspect_links(&app, &q) {
                if let Some(l) = o.data["links"].as_array().into_iter().flatten().find(|l| l["key"] == a["link"]) {
                    for c in l["changed"].as_array().into_iter().flatten() {
                        shown.push_str(&format!("{} {}: before {} after {}\n", c["end"].as_str().unwrap_or(""), c["field"].as_str().unwrap_or(""), sysml_core::agent::text::quote_clip(c["before"].as_str().unwrap_or("")), sysml_core::agent::text::quote_clip(c["after"].as_str().unwrap_or(""))));
                    }
                    shown.push_str(&format!("fingerprint.before: {}\nfingerprint.after: {}\n", l["fingerprint"]["before"].as_str().unwrap_or(""), l["fingerprint"]["after"].as_str().unwrap_or("")));
                }
            }
        }
        return (2, format!("{shown}--since, --fingerprint and --fingerprint-before are required: the fingerprint.after and fingerprint.before that list_suspect_links showed for that --since. Nothing was cleared.\nusage: {CLEAR_USAGE}"));
    };
    a["fingerprint"] = json!(fp);
    a["fingerprint_before"] = json!(fpb);
    match mcp::suspect::clear(&app, &a, &mcp::suspect::Reviewer { route: "command", authenticated: false }) {
        Ok(o) => (0, o.text),
        Err(e) => (1, e.message),
    }
}

/// `sysml-bridge-engine check --workspace DIR [--gates a,b] [--json] [--limit N]`: every gate, one report, exit 0 only without errors.
/// The same program as `sysml-check` (crate sysml-check): read only, with limits on the files, and a last line to read aloud.
fn check_command() -> i32 {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let out = sysml_check::run(&args);
    print!("{}", out.text);
    out.code
}
