//! Workspace folder of `.sysml` files.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use sysml_core::api::FileIn;

pub struct Store {
    root: PathBuf,
    /// Held across read, check, write and log so two agents cannot interleave a write.
    write_lock: Mutex<()>,
}

static TMP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Largest id file read (the tools' own, or one an importer wrote), and all importer files together.
const MAX_IDS_FILE_BYTES: u64 = 256 << 20;
const MAX_IDS_ALL_BYTES: u64 = 1 << 30;
/// Most importer id files read, and the deepest folder looked in.
const MAX_IDS_FILES: usize = 64;
const MAX_WALK_DEPTH: usize = 32;
/// Largest baselines file that is read. A baseline line holds the text of the model, so the file is larger
/// than the id file; above this size nothing more is added.
const MAX_LINES_FILE_BYTES: u64 = 256 << 20;
const MAX_SETTINGS_BYTES: u64 = 64 << 10;
const BASELINES_FILE: &str = "baselines.jsonl";
const CSV_MAPS_FILE: &str = "csv-maps.jsonl";
const SUSPECT_FILE: &str = "suspect-reviews.jsonl";
const IGNORE_FILE: &str = "rule-ignores.jsonl";
const LOCK_FILE: &str = "write.lock";

/// The write lock of a workspace, held: see `Store::lock_writes`. The file lock is released first.
pub struct WriteGuard<'a> {
    _file: Option<std::fs::File>,
    _in_process: MutexGuard<'a, ()>,
}

/// The text of a plain file of at most `cap` bytes. A link, a folder or a bigger file gives `None`, and the
/// size is checked before anything is read.
fn read_plain(p: &Path, cap: u64) -> Option<String> {
    use std::io::Read;
    let m = std::fs::symlink_metadata(p).ok()?;
    if !m.file_type().is_file() || m.len() > cap {
        return None;
    }
    let mut s = String::new();
    std::fs::File::open(p).ok()?.take(cap + 1).read_to_string(&mut s).ok()?;
    (s.len() as u64 <= cap).then_some(s)
}

/// The one accepted spelling of a workspace path. Anything else is refused, never rewritten,
/// so every gate and the write itself see the same string.
pub fn canon_path(rel: &str) -> Result<String, String> {
    let bad = |why: &str| {
        let shown: String = rel.chars().take(80).collect();
        Err(format!("invalid path {}: {why}. Use a relative path such as models/vehicle.sysml.", sysml_core::agent::text::quote_claim(&shown)))
    };
    if rel.is_empty() || rel.len() > 240 {
        return bad("empty or too long");
    }
    if rel.chars().any(|c| c.is_control() || c == '\\' || c == ':') {
        return bad("control character, backslash or colon");
    }
    // The rule for a name: no character that does not show, and no space but the plain one.
    if sysml_core::edit::name_fault(rel).is_some() {
        return bad("a character that does not show (format, private-use, unassigned or default-ignorable), or a space other than the plain space");
    }
    if rel.starts_with('/') {
        return bad("absolute path");
    }
    for seg in rel.split('/') {
        if seg.is_empty() || seg.starts_with('.') || seg.ends_with('.') || seg.starts_with(' ') || seg.ends_with(' ') {
            return bad("empty, hidden or dotted segment, or leading/trailing space or dot");
        }
    }
    if !(rel.ends_with(".sysml") || rel.ends_with(".kerml") || rel.ends_with(IDS_SUFFIX) || rel == RENAMES_FILE) {
        return bad("only .sysml, .kerml, .ids.json (lower case) and renames.jsonl files are stored");
    }
    Ok(rel.to_string())
}

impl Store {
    /// Read an exchange file under this workspace without following a symlink.
    pub fn read_reqif_file(&self, rel: &str) -> Result<String, String> {
        if rel.is_empty() || rel.len() > 240 || rel.starts_with('/') || rel.contains('\\') || rel.chars().any(char::is_control)
            || rel.split('/').any(|s| s.is_empty() || s.starts_with('.') || s == "workspace" || s == "reference")
            || !(rel.ends_with(".reqif") || rel.ends_with(".xml")) {
            return Err("file must be a workspace-relative .reqif or .xml path".into());
        }
        let mut path = self.root.clone();
        for part in rel.split('/') {
            path.push(part);
            if std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?.file_type().is_symlink() {
                return Err("ReqIF file path cannot contain symlinks".into());
            }
        }
        read_plain(&path, sysml_core::agent::reqif::MAX_BYTES as u64).ok_or_else(|| format!("ReqIF file is unreadable or longer than {} bytes", sysml_core::agent::reqif::MAX_BYTES))
    }
}

fn is_model_file(p: &Path) -> bool {
    matches!(p.extension().and_then(|e| e.to_str()), Some("sysml") | Some("kerml"))
}

/// A file stored beside the model that is never read as model text: the source-id maps and the log of renames.
pub fn is_layout_path(p: &str) -> bool {
    p.ends_with(IDS_SUFFIX) || p == RENAMES_FILE
}

/// The log of renames, moves and deletes beside the model (`sysml_core::agent::rekey`): never read as a model.
pub const RENAMES_FILE: &str = sysml_core::agent::rekey::RENAMES_FILE;

/// The source-id map an importer may write beside the model (`<name>.ids.json`); `imported_ids` reads it.
pub const IDS_SUFFIX: &str = ".ids.json";

fn is_stored(p: &Path) -> bool {
    is_model_file(p) || p.to_str().map(is_layout_path).unwrap_or(false)
}

/// The text of a file. A file that is not UTF-8 is still read, with U+FFFD where its bytes are not text: it is listed,
/// every gate reports it (`lexer::NOT_TEXT`), and no edit may write it back (`writes::write_with`). Before, such a file
/// was left out, so the gates passed on a model that did not hold it.
/// A link is refused, so a link inside the workspace cannot expose a file outside it.
pub fn read_text(p: &Path) -> Option<String> {
    let bytes = read_bytes_no_link(p)?;
    Some(match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    })
}

/// The bytes of a regular file, or `None` for a link, a folder or a missing file. The file is opened without following
/// a final link (O_NOFOLLOW on Unix), so the check and the read see the same file.
fn read_bytes_no_link(p: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(not(unix))]
    if std::fs::symlink_metadata(p).ok()?.file_type().is_symlink() {
        return None;
    }
    let mut f = opts.open(p).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut b = Vec::new();
    f.read_to_end(&mut b).ok()?;
    Some(b)
}

impl Store {
    /// True when the bytes of the file on disk are not UTF-8. This is the one test of "not text": the bytes, read here,
    /// and not a U+FFFD in the text that `read_text` gives. A file that is valid UTF-8 and holds the character U+FFFD is
    /// text; it fails the gates until the character is removed, and every tool can still repair it.
    pub fn not_utf8(&self, rel: &str) -> bool {
        self.resolve(rel).ok().and_then(|p| read_bytes_no_link(&p)).map(|b| std::str::from_utf8(&b).is_err()).unwrap_or(false)
    }

    pub fn new(root: PathBuf) -> Self {
        Store { root, write_lock: Mutex::new(()) }
    }

    /// The one lock of the workspace: read, check, write and record happen under it. It is the lock of this process and,
    /// when the state folder exists, a lock on the file `.sysml-bridge/write.lock`, so the server and a command
    /// (another process on the same folder) cannot both pass a check and both write. The lock is released when the
    /// guard is dropped. It is not re-entrant: a function that holds the guard calls the `*_locked` form of another.
    pub fn lock_writes(&self) -> WriteGuard<'_> {
        let in_process = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        WriteGuard { _file: self.lock_file(), _in_process: in_process }
    }

    /// The lock file, locked. `None` when the state folder is not there yet (nothing was recorded, so there is no
    /// decision to race), is a link, or the file is not a plain file: a link is never opened.
    fn lock_file(&self) -> Option<std::fs::File> {
        let dir = self.root.join(".sysml-bridge");
        if !std::fs::symlink_metadata(&dir).map(|m| m.file_type().is_dir()).unwrap_or(false) {
            return None;
        }
        let p = dir.join(LOCK_FILE);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if !m.file_type().is_file() => return None,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
        let f = std::fs::OpenOptions::new().create(true).append(true).open(&p).ok()?;
        f.lock().ok()?;
        Some(f)
    }

    /// Append-only write log. The leading dot keeps it out of the model file list.
    fn history_path(&self) -> PathBuf {
        self.root.join(".sysml-bridge").join("history.jsonl")
    }

    /// The write log, read as the other files of the state folder are: the folder may not be a link, and the log is read
    /// only when it is a plain file. (Before round 22 a link in its place was followed.)
    pub fn read_history(&self) -> Vec<Value> {
        if self.state_dir().is_err() {
            return Vec::new();
        }
        let Some(text) = read_plain(&self.history_path(), u64::MAX - 1) else { return Vec::new() };
        text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    /// Add one entry to the log and return its sequence number. Call with the write lock held.
    pub fn append_history(&self, entry: &mut Value) -> Result<u64, String> {
        use std::io::Write;
        // The log is appended to only when the state folder is a folder and the log a plain file, as the baselines and
        // the proposals are: a link in its place would be written through.
        self.state_dir()?;
        match std::fs::symlink_metadata(self.history_path()) {
            Ok(m) if !m.file_type().is_file() => return Err("history.jsonl is a link or not a plain file, so it is left alone. Fix: remove it".into()),
            _ => {}
        }
        // Only the last entry is read: the log grows with every write, and a long run must not re-read all of it.
        let last = std::fs::read_to_string(self.history_path()).ok().and_then(|t| t.lines().rev().find(|l| !l.trim().is_empty()).and_then(|l| serde_json::from_str::<Value>(l).ok()));
        let seq = last.and_then(|e| e["seq"].as_u64()).unwrap_or(0) + 1;
        entry["seq"] = json!(seq);
        let p = self.history_path();
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).map_err(|e| e.to_string())?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        // One write of a whole line; on failure cut back so no half entry stays.
        if let Err(e) = f.write_all(format!("{entry}\n").as_bytes()).and_then(|_| f.sync_data()) {
            let _ = f.set_len(len);
            return Err(e.to_string());
        }
        Ok(seq)
    }

    /// Named baselines: one JSON line each, beside the write log. A baseline holds the files of the state it names, so it
    /// stays readable when the write log no longer leads back to that state.
    pub fn read_baselines(&self) -> Vec<Value> {
        self.read_lines(BASELINES_FILE)
    }

    /// Add one baseline. The file is only ever appended to. Call with the write lock held.
    pub fn append_baseline(&self, entry: &Value) -> Result<(), String> {
        self.append_line(BASELINES_FILE, entry)
    }

    /// The saved column maps of the spreadsheet round trip: one line each, the last line of a name wins.
    pub fn read_csv_maps(&self) -> Vec<Value> {
        self.read_lines(CSV_MAPS_FILE)
    }

    /// Save a column map by name. The file is only ever appended to. Call with the write lock held.
    pub fn append_csv_map(&self, entry: &Value) -> Result<(), String> {
        self.append_line(CSV_MAPS_FILE, entry)
    }

    /// The reviews of suspect links: one line each, written only by the act of a person, beside the write log and never in the
    /// model text. The file is only ever appended to.
    pub fn read_suspect_reviews(&self) -> Vec<Value> {
        self.read_lines(SUSPECT_FILE)
    }

    /// The rule results a person set aside, one line each, written only by the act of a person.
    pub fn read_rule_ignores(&self) -> Vec<Value> {
        self.read_lines(IGNORE_FILE)
    }

    /// Add one ignored rule result. Call with the write lock held.
    pub fn append_rule_ignore(&self, entry: &Value) -> Result<(), String> {
        self.append_line(IGNORE_FILE, entry)
    }

    /// Add one review of a suspect link. Call with the write lock held.
    pub fn append_suspect_review(&self, entry: &Value) -> Result<(), String> {
        self.append_line(SUSPECT_FILE, entry)
    }

    /// A workspace setting of `.sysml-bridge/settings.json` (for example `{"budget_root": "..."}`). A
    /// file that is missing, a link, too large or not JSON sets nothing.
    pub fn setting(&self, key: &str) -> Option<Value> {
        self.state_dir().ok()?;
        let text = read_plain(&self.root.join(".sysml-bridge").join("settings.json"), MAX_SETTINGS_BYTES)?;
        serde_json::from_str::<Value>(&text).ok()?.get(key).cloned()
    }

    /// The lines of a file of the state folder, as the id file is read: the folder may not be a link, and the file is
    /// read only when it is a plain file of at most `MAX_LINES_FILE_BYTES` (`read_plain` checks before it reads). A file
    /// that fails this is not followed and not read: it gives no line.
    fn read_lines(&self, name: &str) -> Vec<Value> {
        let Ok(dir) = self.state_dir() else { return Vec::new() };
        let Some(text) = read_plain(&dir.join(name), MAX_LINES_FILE_BYTES) else { return Vec::new() };
        text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    /// Add one line to a file of the state folder. Refused when the folder or the file is a link or not a plain file, and
    /// when the file would pass the size that is read back: a line that could not be read again must not be written.
    fn append_line(&self, name: &str, entry: &Value) -> Result<(), String> {
        use std::io::Write;
        let dir = self.state_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let p = dir.join(name);
        let line = format!("{entry}\n");
        match std::fs::symlink_metadata(&p) {
            Ok(m) if !m.file_type().is_file() => return Err(format!("{name} is a link or not a plain file, so it is left alone. Fix: remove it")),
            Ok(m) if m.len() + line.len() as u64 > MAX_LINES_FILE_BYTES => return Err(format!("{name} would be larger than {} MiB, the most that is read back, so nothing was added", MAX_LINES_FILE_BYTES >> 20)),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p).map_err(|e| e.to_string())?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        // One write of a whole line; on failure cut back so no half entry stays.
        if let Err(e) = f.write_all(line.as_bytes()).and_then(|_| f.sync_data()) {
            let _ = f.set_len(len);
            return Err(e.to_string());
        }
        Ok(())
    }

    /// The folder that holds the log and the id file. A link or a file in its place is refused: the
    /// tools would write through it to wherever it points.
    fn state_dir(&self) -> Result<PathBuf, String> {
        let d = self.root.join(".sysml-bridge");
        match std::fs::symlink_metadata(&d) {
            Ok(m) if m.file_type().is_dir() => Ok(d),
            Ok(_) => Err("the .sysml-bridge entry of the workspace is a link or a file, not a folder. Fix: remove it".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(d),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Source ids kept by the tools: `{"ids": {id: qualified name | null}}`. Hidden, so it is not a model file.
    fn ids_path(&self) -> PathBuf {
        self.root.join(".sysml-bridge").join("ids.json")
    }

    /// The text of the id file, or `None` when there is none, it is a link, it is not a plain file, or it
    /// is over `MAX_IDS_FILE_BYTES`.
    pub fn read_ids_raw(&self) -> Option<String> {
        self.state_dir().ok()?;
        read_plain(&self.ids_path(), MAX_IDS_FILE_BYTES)
    }

    /// Replace the id file; `None` removes it. Call with the write lock held.
    pub fn write_ids_raw(&self, text: Option<&str>) -> Result<(), String> {
        use std::io::Write;
        let dir = self.state_dir()?;
        let p = self.ids_path();
        // A file that is not a plain file under the limit is not replaced: it may be a link to a file elsewhere,
        // or an id map that the tools could not read and would lose.
        match std::fs::symlink_metadata(&p) {
            Ok(m) if !m.file_type().is_file() || m.len() > MAX_IDS_FILE_BYTES => {
                return Err("ids.json is a link, not a plain file, or larger than the limit, so it is left alone. Fix: remove it".into())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
        match text {
            Some(t) => {
                if t.len() as u64 > MAX_IDS_FILE_BYTES {
                    return Err(format!("ids.json would be larger than {} MiB", MAX_IDS_FILE_BYTES >> 20));
                }
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let tmp = dir.join(format!("ids.tmp{}", std::process::id()));
                // `create_new` fails on anything already at the path, a link included.
                let _ = std::fs::remove_file(&tmp);
                let done = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&tmp)
                    .and_then(|mut f| f.write_all(t.as_bytes()).and_then(|_| f.sync_data()))
                    .and_then(|_| std::fs::rename(&tmp, &p));
                done.map_err(|e| {
                    let _ = std::fs::remove_file(&tmp);
                    e.to_string()
                })
            }
            None => match std::fs::remove_file(&p) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            },
        }
    }

    /// Id maps an importer wrote beside the model (`<name>.ids.json`), read only. Links are not followed,
    /// and what is read is bounded: file count, folder depth, and bytes per file and in all.
    pub fn imported_ids(&self) -> std::collections::BTreeMap<String, String> {
        fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
            if depth > MAX_WALK_DEPTH {
                return;
            }
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                if out.len() >= MAX_IDS_FILES {
                    return;
                }
                // `DirEntry::file_type` does not follow a link, so a link is neither a folder nor a file here.
                let Ok(ft) = e.file_type() else { continue };
                let p = e.path();
                let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
                if name.starts_with('.') {
                    continue;
                }
                if ft.is_dir() {
                    walk(&p, depth + 1, out);
                } else if ft.is_file() && name.ends_with(".ids.json") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(&self.root, 0, &mut files);
        files.sort();
        let mut out = std::collections::BTreeMap::new();
        let mut budget = MAX_IDS_ALL_BYTES;
        for f in files {
            let Some(text) = read_plain(&f, MAX_IDS_FILE_BYTES.min(budget)) else { continue };
            budget -= text.len() as u64;
            let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
            for (k, q) in v["ids"].as_object().into_iter().flatten() {
                if let Some(q) = q.as_str() {
                    out.entry(k.clone()).or_insert_with(|| q.to_string());
                }
            }
        }
        out
    }

    /// Map a workspace-relative path to disk. Only the canonical form from `canon_path` is accepted, and no folder on
    /// the way may be a symlink: `canon_path` refuses `..` by spelling, and this keeps a linked folder from leading a
    /// read, write or delete out of the workspace. A final link is refused by `read_text` and never listed by `walk_to`;
    /// a write or delete acts on the link itself, not its target.
    fn resolve(&self, rel: &str) -> Result<PathBuf, String> {
        let canon = canon_path(rel)?;
        let mut p = self.root.clone();
        let segs: Vec<&str> = canon.split('/').collect();
        for seg in &segs[..segs.len() - 1] {
            p.push(seg);
            if std::fs::symlink_metadata(&p).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
                return Err(format!("invalid path '{rel}': the folder '{seg}' is a symlink. Use a real folder inside the workspace."));
            }
        }
        p.push(segs[segs.len() - 1]);
        Ok(p)
    }

    /// True when a file is on disk, even if it cannot be read as text.
    pub fn exists(&self, rel: &str) -> bool {
        self.resolve(rel).map(|p| std::fs::symlink_metadata(p).is_ok()).unwrap_or(false)
    }

    fn walk(&self, dir: &Path, out: &mut Vec<PathBuf>) {
        self.walk_to(dir, 0, out);
    }

    /// A link is never followed: a link to a folder is not entered (two links to `.` would be 2^40 paths), and a link to
    /// a file is not listed, so no link can bring a file from outside the workspace into the model.
    fn walk_to(&self, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > MAX_WALK_DEPTH {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let hidden = p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.')).unwrap_or(true);
            if hidden {
                continue;
            }
            let ft = e.file_type().ok();
            let is_real_dir = ft.map(|t| t.is_dir()).unwrap_or(false);
            let is_real_file = ft.map(|t| t.is_file()).unwrap_or(false);
            if is_real_dir {
                self.walk_to(&p, depth + 1, out);
            } else if (is_stored(&p) || (depth == 0 && p.file_name().is_some_and(|n| n == RENAMES_FILE))) && is_real_file {
                out.push(p);
            }
        }
    }

    fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root).unwrap_or(p).to_string_lossy().replace('\\', "/")
    }

    pub fn read_all(&self) -> Vec<FileIn> {
        let mut files = Vec::new();
        self.walk(&self.root, &mut files);
        files.sort();
        files
            .into_iter()
            .filter_map(|p| Some(FileIn { path: self.rel(&p), text: read_text(&p)? }))
            .collect()
    }

    pub fn read(&self, rel: &str) -> Result<String, String> {
        read_text(&self.resolve(rel)?).ok_or_else(|| format!("no file '{rel}'"))
    }

    pub fn write(&self, rel: &str, text: &str) -> Result<(), String> {
        let p = self.resolve(rel)?;
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Write then rename, so a reader never sees a half-written file.
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("file");
        let tmp = p.with_file_name(format!(".{name}.{}.{}.tmp", std::process::id(), TMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let done = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &p));
        if let Err(e) = done {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.to_string());
        }
        Ok(())
    }

    pub fn delete(&self, rel: &str) -> Result<(), String> {
        let p = self.resolve(rel)?;
        std::fs::remove_file(&p).map_err(|_| format!("no file '{rel}'"))?;
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod symlink_tests {
    use super::Store;

    #[test]
    fn a_symlinked_folder_cannot_lead_a_write_read_or_delete_out_of_the_workspace() {
        let base = std::env::temp_dir().join(format!("sysml-store-link-{}", std::process::id()));
        let (ws, outside) = (base.join("ws"), base.join("outside"));
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("victim.sysml"), "package V;").unwrap();
        std::os::unix::fs::symlink(&outside, ws.join("models")).unwrap();
        let store = Store::new(ws.clone());

        assert!(store.write("models/evil.sysml", "package E;").unwrap_err().contains("symlink"));
        assert!(!outside.join("evil.sysml").exists(), "the write must not land outside the workspace");
        assert!(store.read("models/victim.sysml").unwrap_err().contains("symlink"));
        assert!(store.delete("models/victim.sysml").unwrap_err().contains("symlink"));
        assert!(outside.join("victim.sysml").exists(), "the delete must not reach outside the workspace");

        // A link to a file outside is neither read nor listed.
        std::os::unix::fs::symlink(outside.join("victim.sysml"), ws.join("leak.sysml")).unwrap();
        assert!(store.read("leak.sysml").is_err(), "a final link must not be read");
        assert!(!store.not_utf8("leak.sysml"));
        assert!(store.read_all().iter().all(|f| f.path != "leak.sysml"), "a link must not be listed");

        // A real folder still works.
        store.write("real/ok.sysml", "package Ok;").unwrap();
        assert_eq!(store.read("real/ok.sysml").unwrap(), "package Ok;");
        let _ = std::fs::remove_dir_all(&base);
    }
}
