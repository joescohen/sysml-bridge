//! Earley recogniser over tokens.
//!
//! It answers one question: can the grammar derive this token sequence? It builds
//! no tree, so the ambiguity of the grammar file (operator precedence is given in
//! a table, not in the rules) does not matter.
//!
//! Predicted items are not stored. A set keeps the set of nonterminals predicted
//! at its position; the grammar tables say which rules those can start. A set
//! stores only items that have consumed at least one token and now wait for a
//! nonterminal. That keeps memory near a few items per token.

use crate::grammar::{Grammar, END};
use std::collections::HashMap;

pub enum Outcome {
    Accept,
    /// No derivation continues past `token` (index; equal to the token count for
    /// end of text). `allowed` is every terminal the grammar permits there.
    Reject {
        token: usize,
        allowed: Vec<u16>,
    },
    /// A limit was reached before an answer.
    Stopped(&'static str),
}

#[derive(Clone, Copy)]
struct Waiting {
    nt: u16,
    dotted: u32,
    origin: u32,
}

/// Set of 64-bit keys that is emptied in constant time (the working set is
/// emptied once per token).
struct KeySet {
    keys: Vec<u64>,
    stamps: Vec<u32>,
    stamp: u32,
    len: usize,
}

impl KeySet {
    fn new() -> KeySet {
        KeySet {
            keys: vec![0; 1024],
            stamps: vec![0; 1024],
            stamp: 1,
            len: 0,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.stamp = self.stamp.wrapping_add(1);
        if self.stamp == 0 {
            self.stamps.iter_mut().for_each(|s| *s = 0);
            self.stamp = 1;
        }
    }

    fn slot(&self, key: u64) -> usize {
        (key.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 20) as usize & (self.keys.len() - 1)
    }

    /// True when the key was not in the set.
    fn insert(&mut self, key: u64) -> bool {
        let mut at = self.slot(key);
        while self.stamps[at] == self.stamp {
            if self.keys[at] == key {
                return false;
            }
            at = (at + 1) & (self.keys.len() - 1);
        }
        self.keys[at] = key;
        self.stamps[at] = self.stamp;
        self.len += 1;
        if self.len * 2 > self.keys.len() {
            let old_keys = std::mem::replace(&mut self.keys, vec![0; self.stamps.len() * 2]);
            let old_stamps = std::mem::replace(&mut self.stamps, vec![0; old_keys.len() * 2]);
            for (key, stamp) in old_keys.into_iter().zip(old_stamps) {
                if stamp == self.stamp {
                    let mut at = self.slot(key);
                    while self.stamps[at] == self.stamp {
                        at = (at + 1) & (self.keys.len() - 1);
                    }
                    self.keys[at] = key;
                    self.stamps[at] = self.stamp;
                }
            }
        }
        true
    }
}

struct Run {
    /// Items of the set under construction: (dotted rule position, origin set).
    items: Vec<(u32, u32)>,
    seen: KeySet,
    steps: u64,
}

impl Run {
    fn push(&mut self, dotted: u32, origin: u32) {
        self.steps += 1;
        if self.seen.insert(((dotted as u64) << 32) | origin as u64) {
            self.items.push((dotted, origin));
        }
    }

    fn reset(&mut self) {
        self.items.clear();
        self.seen.clear();
    }
}

fn has(set: &[u64], nt: u16) -> bool {
    set[nt as usize / 64] & (1 << (nt % 64)) != 0
}

pub fn recognise(
    g: &Grammar,
    kinds: &[u16],
    max_steps: u64,
    max_items: usize,
    steps_out: &mut u64,
) -> Outcome {
    let n = kinds.len();
    let mut waiting: Vec<Waiting> = Vec::new();
    // Set j owns waiting[waiting_start[j]..waiting_start[j + 1]], sorted by `nt`.
    let mut waiting_start: Vec<u32> = Vec::with_capacity(n + 2);
    waiting_start.push(0);
    let mut predicted_sets: Vec<Vec<u64>> = Vec::new();
    let mut predicted_ids: HashMap<Vec<u64>, u32> = HashMap::new();
    let mut predicted_of: Vec<u32> = Vec::with_capacity(n + 1);
    let mut run = Run {
        items: Vec::new(),
        seen: KeySet::new(),
        steps: 0,
    };
    let mut next: Vec<(u32, u32)> = Vec::new();
    let mut predicted = vec![0u64; g.words];
    run.push(g.start_item, 0);

    for i in 0..=n {
        // Close the set: advance over empty nonterminals and complete finished rules.
        predicted.iter_mut().for_each(|w| *w = 0);
        let mut k = 0;
        while k < run.items.len() {
            let (dotted, origin) = run.items[k];
            k += 1;
            let sym = g.sym_at[dotted as usize];
            if sym == END {
                // A rule that ends where it began matched nothing; those cases are
                // covered by the advance over empty nonterminals below.
                if (origin as usize) < i {
                    let done = g.lhs_at[dotted as usize];
                    let key = (1u64 << 63) | ((done as u64) << 32) | origin as u64;
                    run.steps += 1;
                    if !run.seen.insert(key) {
                        continue;
                    }
                    let from = waiting_start[origin as usize] as usize;
                    let to = waiting_start[origin as usize + 1] as usize;
                    let slice = &waiting[from..to];
                    let first = slice.partition_point(|w| w.nt < done);
                    for w in &slice[first..] {
                        if w.nt != done {
                            break;
                        }
                        run.push(w.dotted + 1, w.origin);
                    }
                    let origin_predicted = &predicted_sets[predicted_of[origin as usize] as usize];
                    for (head, after) in &g.left_parent[done as usize] {
                        run.steps += 1;
                        if has(origin_predicted, *head) {
                            run.push(*after, origin);
                        }
                    }
                }
            } else if sym >= g.nt_base {
                let nt = (sym - g.nt_base) as usize;
                for (w, bits) in predicted.iter_mut().zip(&g.left_closure[nt]) {
                    *w |= *bits;
                }
                if g.nullable[nt] {
                    run.push(dotted + 1, origin);
                }
            }
            if run.steps > max_steps {
                *steps_out = run.steps;
                return Outcome::Stopped("work budget used up");
            }
        }

        // Keep what later completions need from this set.
        let from = waiting.len();
        // Storing and scanning the set costs one more step per item.
        run.steps += run.items.len() as u64;
        for (dotted, origin) in &run.items {
            let sym = g.sym_at[*dotted as usize];
            if sym != END && sym >= g.nt_base {
                waiting.push(Waiting {
                    nt: (sym - g.nt_base) as u16,
                    dotted: *dotted,
                    origin: *origin,
                });
            }
        }
        waiting[from..].sort_by_key(|w| w.nt);
        waiting_start.push(waiting.len() as u32);
        if waiting.len() > max_items {
            *steps_out = run.steps;
            return Outcome::Stopped("item limit reached");
        }
        let id = match predicted_ids.get(&predicted) {
            Some(id) => *id,
            None => {
                let id = predicted_sets.len() as u32;
                predicted_sets.push(predicted.clone());
                predicted_ids.insert(predicted.clone(), id);
                id
            }
        };
        predicted_of.push(id);

        let allowed = |run: &Run| {
            let mut out: Vec<u16> = Vec::new();
            for (dotted, _) in &run.items {
                let sym = g.sym_at[*dotted as usize];
                if sym < g.nt_base {
                    out.push(sym as u16);
                }
            }
            for (terminal, starts) in g.first_term.iter().enumerate() {
                if starts.iter().any(|(head, _)| has(&predicted, *head)) {
                    out.push(terminal as u16);
                }
            }
            out.sort_unstable();
            out.dedup();
            out
        };

        if i == n {
            *steps_out = run.steps;
            return if run.items.contains(&(g.accept_item, 0)) {
                Outcome::Accept
            } else {
                Outcome::Reject {
                    token: n,
                    allowed: allowed(&run),
                }
            };
        }

        // Scan token i into the next set.
        let token = kinds[i] as u32;
        next.clear();
        for (dotted, origin) in &run.items {
            if g.sym_at[*dotted as usize] == token {
                next.push((*dotted + 1, *origin));
            }
        }
        if let Some(starts) = g.first_term.get(token as usize) {
            for (head, after) in starts {
                if has(&predicted, *head) {
                    next.push((*after, i as u32));
                }
            }
            run.steps += starts.len() as u64;
        }
        if next.is_empty() {
            *steps_out = run.steps;
            return Outcome::Reject {
                token: i,
                allowed: allowed(&run),
            };
        }
        run.reset();
        for (dotted, origin) in next.drain(..) {
            run.push(dotted, origin);
        }
    }
    unreachable!("the loop returns at the last set")
}
