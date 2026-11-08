//! Line diffs for write previews and the write log. A hunk list is exact
//! enough to undo a change, so the log needs no copy of the old file.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// Zero-based index of the first old line this hunk touches.
    pub old_start: usize,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

/// Largest middle section, in table cells, that gets a full line-by-line match.
/// Beyond it the whole middle is one hunk: bigger, but still correct.
const MAX_CELLS: usize = 1_000_000;

fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

pub fn hunks(old: &str, new: &str) -> Vec<Hunk> {
    let (a, b) = (lines(old), lines(new));
    let mut p = 0;
    while p < a.len() && p < b.len() && a[p] == b[p] {
        p += 1;
    }
    let mut s = 0;
    while s < a.len() - p && s < b.len() - p && a[a.len() - 1 - s] == b[b.len() - 1 - s] {
        s += 1;
    }
    let (ma, mb) = (&a[p..a.len() - s], &b[p..b.len() - s]);
    let own = |x: &[&str]| x.iter().map(|l| l.to_string()).collect::<Vec<_>>();
    if ma.is_empty() && mb.is_empty() {
        return Vec::new();
    }
    if ma.is_empty() || mb.is_empty() || (ma.len() + 1) * (mb.len() + 1) > MAX_CELLS {
        return vec![Hunk { old_start: p, removed: own(ma), added: own(mb) }];
    }
    // Longest common subsequence table, filled from the end.
    let (n, m) = (ma.len(), mb.len());
    let w = m + 1;
    let mut t = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i * w + j] = if ma[i] == mb[j] { t[(i + 1) * w + j + 1] + 1 } else { t[(i + 1) * w + j].max(t[i * w + j + 1]) };
        }
    }
    let mut out: Vec<Hunk> = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut cur: Option<Hunk> = None;
    while i < n || j < m {
        if i < n && j < m && ma[i] == mb[j] {
            out.extend(cur.take());
            i += 1;
            j += 1;
            continue;
        }
        let h = cur.get_or_insert(Hunk { old_start: p + i, removed: vec![], added: vec![] });
        if j < m && (i == n || t[i * w + j + 1] >= t[(i + 1) * w + j]) {
            h.added.push(mb[j].to_string());
            j += 1;
        } else {
            h.removed.push(ma[i].to_string());
            i += 1;
        }
    }
    out.extend(cur);
    out
}

/// Unified diff text with three lines of context. `old`/`new` of `None` mean the file is absent.
pub fn unified(path: &str, old: Option<&str>, new: Option<&str>, hs: &[Hunk]) -> String {
    if hs.is_empty() {
        return String::new();
    }
    const CTX: usize = 3;
    let old_text = old.unwrap_or("");
    let a = lines(old_text);
    let mut out = String::new();
    out.push_str(&format!("--- {}\n", if old.is_some() { format!("a/{path}") } else { "/dev/null".into() }));
    out.push_str(&format!("+++ {}\n", if new.is_some() { format!("b/{path}") } else { "/dev/null".into() }));
    let put = |out: &mut String, sign: char, l: &str| {
        out.push(sign);
        out.push_str(l.strip_suffix('\n').unwrap_or(l));
        out.push('\n');
        if !l.ends_with('\n') {
            out.push_str("\\ No newline at end of file\n");
        }
    };
    let mut delta: isize = 0;
    let mut k = 0;
    while k < hs.len() {
        // Merge hunks whose context would touch.
        let mut end = k;
        while end + 1 < hs.len() && hs[end + 1].old_start <= hs[end].old_start + hs[end].removed.len() + 2 * CTX {
            end += 1;
        }
        let first = &hs[k];
        let from = first.old_start.saturating_sub(CTX);
        let last = &hs[end];
        let to = (last.old_start + last.removed.len() + CTX).min(a.len());
        let mut body = String::new();
        let (mut old_n, mut new_n) = (0usize, 0usize);
        let mut pos = from;
        for h in &hs[k..=end] {
            for l in &a[pos..h.old_start] {
                put(&mut body, ' ', l);
                old_n += 1;
                new_n += 1;
            }
            for l in &h.removed {
                put(&mut body, '-', l);
                old_n += 1;
            }
            for l in &h.added {
                put(&mut body, '+', l);
                new_n += 1;
            }
            pos = h.old_start + h.removed.len();
        }
        for l in &a[pos..to] {
            put(&mut body, ' ', l);
            old_n += 1;
            new_n += 1;
        }
        let new_from = (from as isize + delta) as usize;
        let show = |start: usize, n: usize| if n == 0 { start } else { start + 1 };
        out.push_str(&format!("@@ -{},{} +{},{} @@\n", show(from, old_n), old_n, show(new_from, new_n), new_n));
        out.push_str(&body);
        for h in &hs[k..=end] {
            delta += h.added.len() as isize - h.removed.len() as isize;
        }
        k = end + 1;
    }
    out
}

/// Undo `hs` on `current`, which must be the text the hunks produced.
pub fn reverse(current: &str, hs: &[Hunk]) -> Result<String, String> {
    let cur = lines(current);
    let mut out = String::new();
    let mut pos = 0usize;
    let mut delta: isize = 0;
    for h in hs {
        let at = (h.old_start as isize + delta) as usize;
        if at < pos || at + h.added.len() > cur.len() || cur[at..at + h.added.len()].iter().zip(&h.added).any(|(x, y)| x != y) {
            return Err("the file no longer matches the logged change".into());
        }
        for l in &cur[pos..at] {
            out.push_str(l);
        }
        for l in &h.removed {
            out.push_str(l);
        }
        pos = at + h.added.len();
        delta += h.added.len() as isize - h.removed.len() as isize;
    }
    for l in &cur[pos..] {
        out.push_str(l);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(old: &str, new: &str) {
        let hs = hunks(old, new);
        assert_eq!(reverse(new, &hs).unwrap(), old, "reverse failed for {old:?} -> {new:?}");
    }

    #[test]
    fn hunks_reverse_exactly() {
        roundtrip("", "a\nb\n");
        roundtrip("a\nb\n", "");
        roundtrip("a\nb\nc\n", "a\nX\nc\n");
        roundtrip("a\nb\nc", "a\nb\nc\n");
        roundtrip("a\nb\nc\nd\ne\nf\n", "a\nB\nc\nd\nE\nf\ng\n");
        roundtrip("x\n", "x\n");
    }

    #[test]
    fn unified_shows_change_with_context() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n";
        let new = "1\n2\n3\nFOUR\n5\n6\n7\n8\n";
        let d = unified("f.sysml", Some(old), Some(new), &hunks(old, new));
        assert!(d.contains("--- a/f.sysml") && d.contains("+++ b/f.sysml"));
        assert!(d.contains("@@ -1,7 +1,7 @@"), "{d}");
        assert!(d.contains("-4\n+FOUR\n"));
    }

    #[test]
    fn created_file_diff_uses_dev_null() {
        let d = unified("n.sysml", None, Some("a\n"), &hunks("", "a\n"));
        assert!(d.contains("--- /dev/null") && d.contains("@@ -0,0 +1,1 @@") && d.contains("+a\n"), "{d}");
    }

    #[test]
    fn stale_file_refuses_reverse() {
        let hs = hunks("a\nb\n", "a\nc\n");
        assert!(reverse("a\nzzz\n", &hs).is_err());
    }

    #[test]
    fn no_change_has_no_hunks() {
        assert!(hunks("a\n", "a\n").is_empty());
    }
}
