//! Element ids (short names) decide which elements are the same across two revisions, before names and text do; a dependency
//! and a transition take a short name like any other identification.

use std::collections::BTreeMap;
use sysml_core::agent::change;
use sysml_core::analyze_one;

const BEFORE: &str = "package P {
    part def <e0000a1> A { port x; }
    part def <e0000s1> S {
        part <e0000p1> a : A;
        part <e0000p2> b : A;
        connection <e0000c1> connect a.x to b.x;
    }
    dependency <e0000d1> from S to A;
    state def M { state s1; state s2; transition <e0000t1> first s1 then s2; }
}
";

/// `S` renamed and moved into a new package, a new unnamed connection before the old one with the same text, the old one
/// re-ended, and the dependency reversed.
const AFTER: &str = "package P {
    part def <e0000a1> A { port x; }
    package Q {
        part def <e0000s1> System {
            part <e0000p1> a : A;
            part <e0000p2> b : A;
            connection <e0000c2> connect a.x to b.x;
            connection <e0000c1> connect b.x to a.x;
        }
    }
    dependency <e0000d1> from A to Q::System;
    state def M { state s1; state s2; transition <e0000t1> first s2 then s1; }
}
";

fn by_short<'a>(m: &'a sysml_core::Model, s: &str) -> usize {
    m.elements.iter().find(|e| e.short_name.as_deref() == Some(s)).unwrap_or_else(|| panic!("no element <{s}>")).id
}

#[test]
fn a_dependency_and_a_transition_keep_their_short_names() {
    let m = analyze_one("p.sysml", BEFORE);
    assert!(m.diagnostics.iter().all(|d| d.severity != sysml_core::Severity::Error), "{:?}", m.diagnostics);
    assert_eq!(m.elements[by_short(&m, "e0000d1")].kind, "dependency");
    assert_eq!(m.elements[by_short(&m, "e0000t1")].kind, "transition");
}

#[test]
fn elements_with_the_same_id_pair_whatever_their_names_place_and_text() {
    let (b, a) = (analyze_one("p.sysml", BEFORE), analyze_one("p.sysml", AFTER));
    let pairs = change::paired_elements(&b, &a, &BTreeMap::new(), &BTreeMap::new());
    for id in ["e0000s1", "e0000p1", "e0000p2", "e0000c1", "e0000d1", "e0000t1"] {
        assert_eq!(pairs.get(&by_short(&b, id)), Some(&by_short(&a, id)), "<{id}>");
    }
    // The new connection has the old one's text, but not its id: it is new.
    assert!(!pairs.values().any(|x| *x == by_short(&a, "e0000c2")));
}

#[test]
fn a_source_id_pairs_elements_that_carry_no_short_name() {
    let b = analyze_one("p.sysml", "package P { part def A; part def S { part a : A; connect a to a; } }");
    let a = analyze_one("p.sysml", "package P { part def A; part def T { part a : A; connect a to a; } }");
    let ids = |q: &str| BTreeMap::from([("_s".to_string(), format!("P::{q}")), ("_c".to_string(), format!("P::{q}::$2"))]);
    let pairs = change::paired_elements(&b, &a, &ids("S"), &ids("T"));
    let find = |m: &sysml_core::Model, q: &str| m.elements.iter().find(|e| e.qualified_name == q).unwrap().id;
    assert_eq!(pairs.get(&find(&b, "P::S")), Some(&find(&a, "P::T")));
    assert_eq!(pairs.get(&find(&b, "P::S::$2")), Some(&find(&a, "P::T::$2")));
}
