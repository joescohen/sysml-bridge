//! The SysML v2 checker on its own. It reads a folder of `.sysml` and `.kerml` files, runs every gate and prints a report whose
//! last line a person can read aloud. It links the parser, the gates and the official-grammar recogniser, and nothing else: no
//! server, no network, no store. It never writes: a folder is opened for reading only.
//!
//! Fail closed: a file over a limit, a link, an unreadable file, an empty folder and a gate that fails inside are each a named
//! failure. None of them is a skip that lets `CHECK PASS` print.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use sysml_core::agent::rules::{self, OfficialVerdict};
use sysml_core::api::FileIn;
use sysml_core::model::Model;


mod fixtures {
    include!(concat!(env!("OUT_DIR"), "/fixtures.rs"));
}

/// Largest model file read, in bytes. Larger is a named failure, before the file is read.
pub const MAX_FILE_BYTES: u64 = 4 << 20;
/// Most bytes of all files of one run.
pub const MAX_RUN_BYTES: u64 = 64 << 20;
/// Most model files in one run.
pub const MAX_FILES: usize = 2000;
/// Deepest folder level entered below the folder given.
pub const MAX_DEPTH: usize = 16;
/// Longest text argument (a gate list, a rule id).
const MAX_ARG: usize = 500;
/// Stack of the thread that runs the gates: a deep model must not end the program with a crash.
pub(crate) const STACK_BYTES: usize = 256 << 20;

/// What the program prints and how it ends.
pub struct Outcome {
    pub text: String,
    pub code: i32,
}

/// A problem with the input, found before any gate ran. Each is an error of the check.
#[derive(Debug, Clone)]
pub struct Failure {
    pub path: String,
    pub rule: &'static str,
    pub message: String,
    pub fix: &'static str,
}

/// The rules of the input stage, with the same fields as a rule of a gate.
pub const INPUT_RULES: [(&str, &str); 8] = [
    ("input-file-too-large", "A model file is larger than the limit (4 MiB). It is named and the check fails; it is never skipped"),
    ("input-run-too-large", "The model files together are larger than the limit (64 MiB)"),
    ("input-too-many-files", "The folder holds more model files than the limit (2000)"),
    ("input-too-deep", "A folder is nested deeper than the limit (16 levels)"),
    ("input-unreadable", "A model file could not be read"),
    ("input-link", "A link (symbolic link) is not followed: a model file must be a plain file in the folder"),
    ("input-no-files", "The folder holds no .sysml or .kerml file, so there was nothing to check"),
    ("check-did-not-run", "A gate failed inside, or the checker stopped; a check that did not run is not a pass"),
];

/// The gate: the model, the gates asked for and the requirements asked for. The program uses `rules::check_all`; a test can give
/// another to prove that a gate that fails inside fails the run.
pub type Gate = dyn Fn(&Model, Option<&[String]>, Option<&[String]>) -> Result<Value, String> + Send + Sync;

fn official(text: &str) -> (OfficialVerdict, u64) {
    use sysml_conform::{Limits, Verdict};
    let report = sysml_conform::Checker::new().check_str(text, &Limits::default());
    let verdict = match report.verdict {
        Verdict::Valid => OfficialVerdict::Valid,
        Verdict::NotChecked(why) => OfficialVerdict::NotChecked(why),
        Verdict::Invalid(f) => OfficialVerdict::Invalid { line: f.line, column: f.column, found: f.found, found_kind: f.found_kind, allowed: f.allowed },
    };
    (verdict, report.steps)
}

/// Text that is safe to print: a control character from a file name or a message (a terminal escape) becomes `?`.
pub fn plain(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { '?' } else { c }).collect()
}

pub(crate) fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n).collect::<String>())
    }
}

fn fail(path: &str, rule: &'static str, message: String, fix: &'static str) -> Failure {
    Failure { path: path.to_string(), rule, message, fix }
}

/// Opens the plain file `rel` under `root` for reading. No step of `rel` may be a link: on Unix each step is opened from the
/// folder before it with `O_NOFOLLOW`, so a folder or a file that is swapped for a link while the run goes on is refused by the
/// system itself, not by a check that could be raced. `root` is the folder the person named and is opened as given.
/// What a name in a folder is, read without following a link.
#[cfg(unix)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Link,
    Dir,
    File,
    Other,
}

#[cfg(unix)]
fn not_a_name(m: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, m)
}

/// Opens the folder `rel` under `root` ("" is `root`). Each step is opened from the folder before it with `O_NOFOLLOW`.
#[cfg(unix)]
fn open_dir_inside(root: &Path, rel: &Path) -> std::io::Result<std::fs::File> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};
    let mut dir = std::fs::File::open(root)?;
    for c in rel.components() {
        let std::path::Component::Normal(step) = c else { return Err(not_a_name("the path holds a step that is not a name")) };
        let name = CString::new(step.as_bytes()).map_err(|_| not_a_name("the name holds a zero byte"))?;
        // SAFETY: `dir` is an open descriptor, `name` is a valid C string, and a returned descriptor is owned by the new `File`.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        dir = unsafe { std::fs::File::from_raw_fd(fd) };
    }
    Ok(dir)
}

/// Opens the plain file `rel` under `root` for reading. No step of `rel` may be a link: on Unix each step is opened from the
/// folder before it with `O_NOFOLLOW`, so a folder or a file that is swapped for a link while the run goes on is refused by the
/// system itself, not by a check that could be raced. `root` is the folder the person named and is opened as given.
#[cfg(unix)]
fn open_inside(root: &Path, rel: &Path) -> std::io::Result<std::fs::File> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};
    let last = match rel.components().next_back() {
        Some(std::path::Component::Normal(n)) => n,
        _ => return Err(not_a_name("no file name")),
    };
    let dir = open_dir_inside(root, rel.parent().unwrap_or(Path::new("")))?;
    let name = CString::new(last.as_bytes()).map_err(|_| not_a_name("the name holds a zero byte"))?;
    // O_NONBLOCK: a name that became a pipe or a device must not make the run wait; the type is checked on the open file below.
    // SAFETY: as in `open_dir_inside`.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let f = unsafe { std::fs::File::from_raw_fd(fd) };
    if !f.metadata()?.file_type().is_file() {
        return Err(not_a_name("not a plain file"));
    }
    Ok(f)
}

/// Sets the error number of this thread (the systems this is built for name the place differently).
#[cfg(unix)]
fn set_errno(v: libc::c_int) {
    // SAFETY: each call gives the address of this thread's own error number, which is always valid to write.
    #[cfg(any(target_os = "linux", target_os = "emscripten", target_os = "hurd"))]
    unsafe {
        *libc::__errno_location() = v;
    }
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "dragonfly"))]
    unsafe {
        *libc::__error() = v;
    }
    #[cfg(any(target_os = "android", target_os = "openbsd", target_os = "netbsd"))]
    unsafe {
        *libc::__errno() = v;
    }
    #[cfg(not(any(target_os = "linux", target_os = "emscripten", target_os = "hurd", target_os = "macos", target_os = "ios", target_os = "freebsd", target_os = "dragonfly", target_os = "android", target_os = "openbsd", target_os = "netbsd")))]
    compile_error!("sysml-check: no way to clear the error number on this system, so a folder read error could not be told from the end of the folder");
}

/// The names in the folder `rel` under `root`, each with what it is. The folder is reached as in `open_dir_inside` and the names
/// are read from that open folder, not from its path: a folder that is swapped for a link while the run goes on cannot make the
/// run list (or read) another folder. An entry that cannot be read is an `Err` in the list, never a name that is left out.
#[cfg(unix)]
fn list_inside(root: &Path, rel: &Path) -> std::io::Result<Vec<std::io::Result<(std::ffi::OsString, Kind)>>> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::{AsRawFd, IntoRawFd};
    let dir = open_dir_inside(root, rel)?;
    // A second descriptor of the same open folder for the type of each name (`fstatat`); `fdopendir` owns the first.
    let at = dir.try_clone()?;
    // SAFETY: the descriptor is open and is given to the stream, which closes it in `closedir`.
    let stream = unsafe { libc::fdopendir(dir.into_raw_fd()) };
    if stream.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let mut out = Vec::new();
    loop {
        // `readdir` gives a null pointer at the end of the folder AND on an error; only the error number tells them apart, so it
        // is cleared first. An error is a failure of the run, never an early "end of folder" that leaves files out.
        // SAFETY: `stream` is a valid open stream until `closedir` below; the entry is copied before the next call.
        set_errno(0);
        let e = unsafe { libc::readdir(stream) };
        if e.is_null() {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error().unwrap_or(0) != 0 {
                unsafe { libc::closedir(stream) };
                return Err(err);
            }
            break;
        }
        let name = unsafe { std::ffi::CStr::from_ptr((*e).d_name.as_ptr()) };
        let bytes = name.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        // SAFETY: `st` is written by a call that succeeds; it is read only then.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        let rc = unsafe { libc::fstatat(at.as_raw_fd(), name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) };
        let os = std::ffi::OsStr::from_bytes(bytes).to_os_string();
        if rc != 0 {
            out.push(Err(std::io::Error::last_os_error()));
            continue;
        }
        let kind = match st.st_mode & libc::S_IFMT {
            libc::S_IFLNK => Kind::Link,
            libc::S_IFDIR => Kind::Dir,
            libc::S_IFREG => Kind::File,
            _ => Kind::Other,
        };
        out.push(Ok((os, kind)));
    }
    unsafe { libc::closedir(stream) };
    Ok(out)
}

/// Not Unix: there is no open that refuses links one step at a time here, so this is weaker and it fails closed. The file is
/// read only when the name is a plain file and the full path, with every link resolved, is still the same path under `root`.
#[cfg(not(unix))]
fn open_inside(root: &Path, rel: &Path) -> std::io::Result<std::fs::File> {
    let bad = |m: &'static str| std::io::Error::new(std::io::ErrorKind::InvalidInput, m);
    if !rel.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return Err(bad("the path holds a step that is not a name"));
    }
    let path = root.join(rel);
    let want = std::fs::canonicalize(root)?.join(rel);
    let f = std::fs::File::open(&path)?;
    if !std::fs::symlink_metadata(&path)?.file_type().is_file() || !f.metadata()?.file_type().is_file() || std::fs::canonicalize(&path)? != want {
        return Err(bad("the file or a folder on its path is a link"));
    }
    Ok(f)
}

#[cfg(not(unix))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Link,
    Dir,
    File,
    Other,
}

/// Not Unix: the names are read by path (weaker; see `open_inside`). A link is still named and never entered.
#[cfg(not(unix))]
fn list_inside(root: &Path, rel: &Path) -> std::io::Result<Vec<std::io::Result<(std::ffi::OsString, Kind)>>> {
    Ok(std::fs::read_dir(root.join(rel))?
        .map(|e| {
            let e = e?;
            let ft = e.file_type()?;
            let kind = if ft.is_symlink() { Kind::Link } else if ft.is_dir() { Kind::Dir } else if ft.is_file() { Kind::File } else { Kind::Other };
            Ok((e.file_name(), kind))
        })
        .collect())
}

/// The model files of a folder, read for reading only, and the problems found on the way. Every limit is checked before the
/// work it bounds. Hidden folders (names that start with a dot, such as `.git` or `.sysml-bridge`) are not entered.
pub fn collect(root: &Path) -> (Vec<FileIn>, Vec<Failure>) {
    let mut files: Vec<FileIn> = Vec::new();
    let mut failures: Vec<Failure> = Vec::new();
    let mut total: u64 = 0;
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    let rel = |p: &Path| p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/");
    let mut stopped = false;
    while let Some((dir, depth)) = stack.pop() {
        let mut entries: Vec<(String, PathBuf, Kind)> = match list_inside(root, dir.strip_prefix(root).unwrap_or(Path::new(""))) {
            Ok(list) => {
                // An entry that cannot be read is a failure of the run, never a file that is silently left out.
                let mut out = Vec::new();
                for e in list {
                    match e {
                        Ok((name, kind)) => out.push((name.to_string_lossy().to_string(), dir.join(&name), kind)),
                        Err(err) => failures.push(fail(&rel(&dir), "input-unreadable", format!("an entry of this folder could not be read: {}", err.kind()), "Check that every file in the folder can be read.")),
                    }
                }
                out
            }
            Err(e) => {
                failures.push(fail(&rel(&dir), "input-unreadable", format!("the folder could not be read: {}", e.kind()), "Check that the folder exists, is not a link, and can be read."));
                continue;
            }
        };
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        // Folders are pushed in reverse so that they are read in name order.
        let mut dirs = Vec::new();
        for (name, path, kind) in entries {
            let is_model = matches!(path.extension().and_then(|e| e.to_str()), Some("sysml") | Some("kerml"));
            if kind == Kind::Link {
                // A link could lead out of the folder or in a circle. It is named, not followed.
                if is_model || path.is_dir() {
                    failures.push(fail(&rel(&path), "input-link", "this is a link; links are not followed".into(), "Replace the link with the file itself, or remove it."));
                }
                continue;
            }
            if kind == Kind::Dir {
                if name.starts_with('.') {
                    continue;
                }
                if depth + 1 > MAX_DEPTH {
                    failures.push(fail(&rel(&path), "input-too-deep", format!("this folder is nested more than {MAX_DEPTH} levels deep and was not read"), "Move the model files nearer to the top folder."));
                } else {
                    dirs.push((path, depth + 1));
                }
                continue;
            }
            if kind != Kind::File || !is_model {
                continue;
            }
            if stopped {
                continue;
            }
            if files.len() >= MAX_FILES {
                failures.push(fail(&rel(&path), "input-too-many-files", format!("the folder holds more than {MAX_FILES} model files; the rest were not read"), "Check the files in smaller folders."));
                stopped = true;
                continue;
            }
            // The entry was a plain file in a plain folder when it was listed. The open does not trust that: see `open_inside`.
            // The size is asked of the open file, so it is the size of the file that is read.
            let opened = open_inside(root, path.strip_prefix(root).unwrap_or(&path)).and_then(|f| f.metadata().map(|m| (f, m.len())));
            let (f, size) = match opened {
                Ok(x) => x,
                Err(e) => {
                    failures.push(fail(&rel(&path), "input-unreadable", format!("the file could not be read: {}", e.kind()), "Check that the file exists, is not a link, and can be read."));
                    continue;
                }
            };
            if size > MAX_FILE_BYTES {
                failures.push(fail(&rel(&path), "input-file-too-large", format!("the file is {} bytes; the limit is {} ({} MiB)", size, MAX_FILE_BYTES, MAX_FILE_BYTES >> 20), "Split the file into smaller files."));
                continue;
            }
            if total + size > MAX_RUN_BYTES {
                failures.push(fail(&rel(&path), "input-run-too-large", format!("the files together would pass {} MiB; this file and the ones after it were not read", MAX_RUN_BYTES >> 20), "Check the files in smaller folders."));
                stopped = true;
                continue;
            }
            // The read itself is held to the limit again: a file that grows while it is read is not trusted.
            let read = {
                use std::io::Read;
                let mut b = Vec::new();
                f.take(MAX_FILE_BYTES + 1).read_to_end(&mut b).map(|_| b)
            };
            match read {
                Ok(b) if b.len() as u64 > MAX_FILE_BYTES => failures.push(fail(&rel(&path), "input-file-too-large", format!("the file grew past {} MiB while it was read", MAX_FILE_BYTES >> 20), "Split the file into smaller files.")),
                Ok(b) => {
                    total += b.len() as u64;
                    // Bytes that are not UTF-8 are read with a replacement character; the syntax gate then reports the file.
                    let text = match String::from_utf8(b) {
                        Ok(t) => t,
                        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
                    };
                    files.push(FileIn { path: rel(&path), text });
                }
                Err(e) => failures.push(fail(&rel(&path), "input-unreadable", format!("the file could not be read: {}", e.kind()), "Check that the file can be read.")),
            }
        }
        stack.extend(dirs.into_iter().rev());
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    if files.is_empty() && failures.is_empty() {
        failures.push(fail(".", "input-no-files", "the folder holds no .sysml or .kerml file".into(), "Give the folder that holds the model files."));
    }
    (files, failures)
}

/// Options of a run.
#[derive(Default)]
pub struct Opts {
    pub dir: Option<String>,
    pub gates: Option<Vec<String>>,
    pub require: Option<Vec<String>>,
    pub json: bool,
    pub limit: usize,
}

const USAGE: &str = "usage: sysml-check [check] DIR [--gates syntax,semantic,traceability,connections,coverage,official,spec] [--require satisfy,verify,source] [--json] [--limit N]\n       sysml-check --list-rules\n       sysml-check --explain RULE\n  Reads the .sysml and .kerml files under DIR (read only), runs every gate and prints a report. The last line is\n  CHECK PASS: <n> files, <n> elements, 0 errors, <n> warnings   or   CHECK FAIL: <n> errors; first: <file>:<line> <rule> <what to fix>\n  Exit code 0: pass. 1: errors found. 2: a bad argument, or the check could not run.";

/// Parse the arguments, run, and return the text and the exit code. This is the whole program.
pub fn run(args: &[String]) -> Outcome {
    run_with(args, &|m, g, r| rules::check_all(m, g, r).map_err(|e| e.message))
}

/// `run` with the gate given.
pub fn run_with(args: &[String], gate: &Gate) -> Outcome {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Outcome { text: format!("{USAGE}\n"), code: 0 };
    }
    if args.iter().any(|a| a.len() > MAX_ARG) {
        return bad("an argument is longer than 500 characters");
    }
    if args.iter().any(|a| a == "--list-rules") {
        return Outcome { text: list_rules(), code: 0 };
    }
    if let Some(i) = args.iter().position(|a| a == "--explain") {
        return match args.get(i + 1) {
            Some(rule) => explain(rule),
            None => bad("--explain needs a rule id; --list-rules prints them"),
        };
    }
    // `sysml-bridge-engine check ...` and older callers put the word `check` first.
    let args = if args.first().map(String::as_str) == Some("check") { &args[1..] } else { args };
    let mut o = Opts { limit: 200, ..Opts::default() };
    let split = |v: &str| v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => o.json = true,
            "--workspace" | "--gates" | "--require" | "--limit" => {
                let Some(v) = args.get(i + 1) else { return bad(&format!("{} needs a value", args[i])) };
                match args[i].as_str() {
                    "--workspace" => o.dir = Some(v.clone()),
                    "--gates" => o.gates = Some(split(v)),
                    "--require" => o.require = Some(split(v)),
                    _ => match v.parse() {
                        Ok(n) => o.limit = n,
                        Err(_) => return bad("--limit must be a number"),
                    },
                }
                i += 1;
            }
            a if a.starts_with("--") => return bad(&format!("unknown argument '{}'", plain(&clip(a, 60)))),
            a => {
                if o.dir.is_some() {
                    return bad("give one folder");
                }
                o.dir = Some(a.to_string());
            }
        }
        i += 1;
    }
    for g in o.gates.iter().flatten() {
        if !rules::GATES.contains(&g.as_str()) {
            return bad(&format!("unknown gate '{}'; the gates are {}", plain(&clip(g, 60)), rules::GATES.join(", ")));
        }
    }
    if o.gates.as_ref().map(|g| g.is_empty()).unwrap_or(false) {
        return bad("--gates names no gate");
    }
    for r in o.require.iter().flatten() {
        if r != "satisfy" && r != "verify" && r != "source" {
            return bad(&format!("--require is satisfy, verify or source, not '{}'", plain(&clip(r, 60))));
        }
    }
    let Some(dir) = o.dir.clone() else { return bad("the folder to check is required") };
    let root = PathBuf::from(&dir);
    if !root.is_dir() {
        return bad(&format!("'{}' is not a folder", plain(&clip(&dir, 200))));
    }
    check_dir(&root, &o, gate)
}

fn bad(why: &str) -> Outcome {
    Outcome { text: format!("{why}\n{USAGE}\n"), code: 2 }
}

/// Check a folder. Reads only. The last line of the text is the line to read aloud.
pub fn check_dir(root: &Path, o: &Opts, gate: &Gate) -> Outcome {
    sysml_core::agent::rules::set_official_checker(official);
    let (mut files, failures) = collect(root);
    let nfiles = files.len();
    // A run that hit the count or size limit is refused as a whole: the gates are not run on part of the input.
    if failures.iter().any(|f| matches!(f.rule, "input-too-many-files" | "input-run-too-large")) {
        files.clear();
    }
    // The gates run in a thread of their own with a large stack. A panic or an error in a gate ends the run as a failure.
    let ran: Result<Result<(usize, Option<Value>), String>, ()> = std::thread::scope(|s| {
        let work = std::thread::Builder::new().stack_size(STACK_BYTES).spawn_scoped(s, || {
            if files.is_empty() {
                return Ok((0, None));
            }
            let mut model = sysml_core::api::to_model(files);
            sysml_core::agent::add_namespace_checks(&mut model);
            let n = model.elements.len();
            let v = gate(&model, o.gates.as_deref(), o.require.as_deref())?;
            Ok((n, Some(v)))
        });
        match work {
            Ok(h) => h.join().map_err(|_| ()),
            Err(_) => Err(()),
        }
    });
    let (elements, got) = match ran {
        Ok(Ok(x)) => x,
        Ok(Err(why)) => return did_not_run(&format!("a gate stopped: {}", plain(&clip(&why, 200))), 2),
        Err(()) => return did_not_run("a gate failed inside (the checker stopped)", 2),
    };
    // The problems of the input are errors of the check, and the first things the report names.
    let input: Vec<Value> = failures.iter().map(|f| json!({ "gate": "input", "rule": f.rule, "severity": "error", "element": null, "file": f.path, "line": 0, "message": f.message, "fix": f.fix })).collect();
    // No report is made up as a pass. With no file read, the report is the empty one and it carries the input errors; without an
    // input error either, nothing explains the empty run and it is a failure to run.
    let mut report = match got {
        Some(v) => match read_report(&v, o.gates.as_deref()) {
            Ok(()) => v,
            Err(why) => return did_not_run(&format!("a gate returned a report that cannot be read: {why}"), 2),
        },
        None if !failures.is_empty() => json!({ "passed": false, "errors": 0, "warnings": 0, "infos": 0, "gates": {}, "findings": [], "by_rule": {} }),
        None => return did_not_run("no file was read and no reason was found", 2),
    };
    // Counts are read only after `read_report` has shown they are present numbers.
    let errors = report["errors"].as_u64().unwrap_or(u64::MAX) + failures.len() as u64;
    let warnings = report["warnings"].as_u64().unwrap_or(0);
    let gates_ok = report["gates"].as_object().map(|g| g.values().all(|c| c["passed"] == true)).unwrap_or(false) || report["gates"].as_object().map(|g| g.is_empty()).unwrap_or(false) && !failures.is_empty();
    report["errors"] = json!(errors);
    report["passed"] = json!(errors == 0 && report["passed"] == true && gates_ok);
    report["input_problems"] = json!(input);
    report["files"] = json!(nfiles);
    report["elements"] = json!(elements);
    let passed = report["passed"] == true;
    let last = if passed {
        format!("CHECK PASS: {nfiles} files, {elements} elements, 0 errors, {warnings} warnings")
    } else {
        let first = input.first().cloned().or_else(|| report["findings"].as_array().and_then(|f| f.iter().find(|x| x["severity"] == "error").cloned()));
        let (file, line, rule, say) = match first {
            Some(f) => (f["file"].as_str().unwrap_or("-").to_string(), f["line"].as_u64().unwrap_or(0), f["rule"].as_str().unwrap_or("-").to_string(), sentence(&f)),
            None => ("-".into(), 0, "check-did-not-run".into(), "run the check again; no error could be named".into()),
        };
        format!("CHECK FAIL: {errors} errors; first: {}:{line} {} {}", plain(&clip(&file, 200)), plain(&rule), say)
    };
    let code = if passed { 0 } else { 1 };
    if o.json {
        // The report as before, with the input problems added; the last line is the report itself.
        return Outcome { text: format!("{}\n", serde_json::to_string_pretty(&report).unwrap_or_default()), code };
    }
    let mut text = String::new();
    text.push_str(&rules::report_text(&report, o.limit));
    for f in &input {
        text.push_str(&format!("[input/{}] error {} : {} Fix: {}\n", plain(f["rule"].as_str().unwrap_or("")), plain(&clip(f["file"].as_str().unwrap_or(""), 200)), plain(&clip(f["message"].as_str().unwrap_or(""), 300)), f["fix"].as_str().unwrap_or("")));
    }
    text.push_str(&last);
    text.push('\n');
    Outcome { text, code }
}

/// A report a gate returned must hold what the run reads: a pass flag, error and warning counts, and for each gate asked for (all
/// of them when none were named) an entry with its own pass flag and error count. A gate that is missing did not run, and a
/// run where a gate did not run fails.
fn read_report(v: &Value, asked: Option<&[String]>) -> Result<(), String> {
    if !v.is_object() || !v["passed"].is_boolean() || !v["errors"].is_u64() || !v["warnings"].is_u64() || !v["findings"].is_array() {
        return Err("it has no pass flag, counts or findings".into());
    }
    let gates = v["gates"].as_object().ok_or("it has no gates")?;
    let wanted: Vec<String> = match asked {
        Some(l) => l.to_vec(),
        None => rules::GATES.iter().map(|g| g.to_string()).collect(),
    };
    for g in &wanted {
        let Some(c) = gates.get(g) else { return Err(format!("the gate {g} did not run")) };
        if !c["passed"].is_boolean() || !c["errors"].is_u64() {
            return Err(format!("the gate {g} has no result"));
        }
    }
    Ok(())
}

/// One plain sentence that says what to fix: the fix of the finding when it is words, else what is wrong.
fn sentence(f: &Value) -> String {
    let fix = f["fix"].as_str().unwrap_or("").trim();
    let msg = f["message"].as_str().unwrap_or("").trim();
    // The fix of a finding is often written for an agent that calls tools; an operator gets the plain message then.
    let for_an_agent = ["validate_model", "create_element", "create_relationship", "update_element", "edit_model", "get_", "apply_edits", "propose_"].iter().any(|t| fix.contains(t));
    let pick = if !fix.is_empty() && !fix.contains('{') && !fix.contains('<') && !for_an_agent { fix } else { msg };
    let one = pick.lines().next().unwrap_or("").trim();
    plain(&clip(one, 220))
}

fn did_not_run(why: &str, code: i32) -> Outcome {
    Outcome { text: format!("The check did not run: {why}\nCHECK FAIL: 1 errors; first: -:0 check-did-not-run run the check again, and send the folder and this output to the person who owns the checker\n"), code }
}

/// `--list-rules`: id, gate and severity of every rule, one per line.
pub fn list_rules() -> String {
    let mut s = String::from("rule\tgate\tseverity\n");
    for r in rules::registry() {
        s.push_str(&format!("{}\t{}\t{}\n", r.id, r.gate, r.severity));
    }
    for (id, _) in INPUT_RULES {
        s.push_str(&format!("{id}\tinput\terror\n"));
    }
    s
}

/// `--explain RULE`: what the rule means, an example that fails, and what the checker says to do about it.
pub fn explain(id: &str) -> Outcome {
    let id = id.trim();
    if let Some((_, meaning)) = INPUT_RULES.iter().find(|(r, _)| *r == id) {
        return Outcome { text: format!("Rule: {id}\nGate: input\nSeverity: error\nMeaning: {meaning}.\nThis rule has no example file: it is about the files of the folder, not about SysML text.\n"), code: 0 };
    }
    let Some(r) = rules::registry().into_iter().find(|r| r.id == id) else {
        return Outcome { text: format!("no rule '{}'. --list-rules prints the rules.\n", plain(&clip(id, 80))), code: 2 };
    };
    let mut s = format!("Rule: {}\nGate: {}\nSeverity: {}\nMeaning: {}.\n", r.id, r.gate, r.severity, r.meaning.trim_end_matches('.'));
    match fixtures::FIXTURES.iter().find(|(n, _)| *n == id) {
        Some((_, text)) => {
            s.push_str("\nAn example that fails:\n");
            for l in text.lines().take(40) {
                s.push_str(&format!("    {l}\n"));
            }
            let mut m = sysml_core::api::to_model(vec![FileIn { path: "example.sysml".into(), text: (*text).to_string() }]);
            sysml_core::agent::add_namespace_checks(&mut m);
            sysml_core::agent::rules::set_official_checker(official);
            let run = rules::run_gates(&m, &[r.gate], None, None, &["satisfy", "verify"]);
            if let Some(f) = run.findings.iter().find(|f| f["rule"] == id) {
                s.push_str(&format!("\nThe checker says: {}\n", plain(&clip(f["message"].as_str().unwrap_or(""), 400))));
                s.push_str(&format!("The fix: {}\n", plain(&clip(f["fix"].as_str().unwrap_or(""), 400))));
            }
        }
        None => s.push_str("\nThis rule has no example file in this build.\n"),
    }
    Outcome { text: s, code: 0 }
}

#[cfg(all(test, unix))]
mod link_swap_tests {
    use super::open_inside;
    use std::path::Path;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sysml-check-{tag}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&d).unwrap();
        d
    }

    /// A plain file in a plain folder opens. The same name as a link does not, and neither does a file under a folder that is a link:
    /// this is the state a swap during the run leaves, and the open itself refuses it.
    #[test]
    fn a_link_at_any_step_of_the_path_is_refused() {
        let d = scratch("steps");
        let (root, outside) = (d.join("root"), d.join("outside"));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("sub/a.sysml"), "package A;").unwrap();
        std::fs::write(outside.join("secret.sysml"), "package Secret;").unwrap();
        assert!(open_inside(&root, Path::new("sub/a.sysml")).is_ok());
        // the last step is a link to a file outside
        std::os::unix::fs::symlink(outside.join("secret.sysml"), root.join("sub/b.sysml")).unwrap();
        assert!(open_inside(&root, Path::new("sub/b.sysml")).is_err());
        // a folder on the path is a link to a folder outside: the file under it is a plain file, and it is still refused
        std::os::unix::fs::symlink(&outside, root.join("linked")).unwrap();
        assert!(std::fs::read_to_string(root.join("linked/secret.sysml")).is_ok(), "the plain open follows the link (the control)");
        assert!(open_inside(&root, Path::new("linked/secret.sysml")).is_err());
        // the listing: a folder that is a link is refused; the plain folder lists its names with what each is
        assert!(super::list_inside(&root, Path::new("linked")).is_err());
        let mut names: Vec<(String, super::Kind)> = super::list_inside(&root, Path::new("sub")).unwrap().into_iter().map(|e| e.unwrap()).map(|(n, k)| (n.to_string_lossy().to_string(), k)).collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(names, vec![("a.sysml".to_string(), super::Kind::File), ("b.sysml".to_string(), super::Kind::Link)]);
        // a step that is not a name, and a folder in place of the file
        assert!(open_inside(&root, Path::new("../outside/secret.sysml")).is_err());
        assert!(open_inside(&root, Path::new("sub")).is_err());
    }
}
