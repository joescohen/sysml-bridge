//! Tools round 30: traceability in Cameo's words (properties and closures), direct against implied, and suspect links.
//! Every fixture is invented (a pump and a gearbox).

#![allow(dead_code, unused_imports)]

use serde_json::{json, Value};
use sysml_core::agent::{self, suspect, trace_props};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

fn model(text: &str) -> Model {
    let mut m = api::to_model(vec![FileIn { path: "m.sysml".into(), text: text.into() }]);
    agent::add_namespace_checks(&mut m);
    m
}

const BASE: &str = "package P {
    part def Pump;
    part def Gearbox;
    part def Unit {
        part pump : Pump;
    }
    part def FastPump :> Pump;
    requirement throughput { doc /* The pump moves 5 litres a minute. */ }
    requirement top;
    requirement mid;
    requirement leaf;
    requirement spare;
    verification check {
        objective {
            verify throughput;
        }
    }
    satisfy throughput by Pump;
    #SysMLv1::DeriveReqt dependency from mid to top;
    #SysMLv1::DeriveReqt dependency from leaf to mid;
    #SysMLv1::Refine dependency from Gearbox to top;
    #SysMLv1::Trace dependency from Gearbox to spare;
    allocate Pump to Gearbox;
}
";

fn props(m: &Model, qn: &str) -> Value {
    let id = m.find(qn).unwrap_or_else(|| panic!("no {qn}")).id;
    trace_props::of(m, &trace_props::index(m), id)
}

fn prop<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["properties"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap_or_else(|| panic!("no property {name}: {v}"))
}

fn refs(p: &Value, key: &str) -> Vec<String> {
    p[key].as_array().unwrap().iter().map(|x| x["ref"].as_str().unwrap().to_string()).collect()
}

fn digest(v: &Value) -> String {
    v["properties"].as_array().unwrap().iter().filter(|p| p["applicable"] == true && (p["count"] != 0 || p["all_count"] != 0)).map(|p| format!("{}={:?}/{:?}", p["name"], refs(p, "items"), refs(p, "all_items"))).collect::<Vec<_>>().join("\n")
}

const IMPLIED: &str = "package P {
    part def Pump;
    part def Motor;
    part def Unit {
        part pump : Pump;
        part motor : Motor;
    }
    requirement speed;
    requirement torque;
    requirement load;
    satisfy speed by Unit::pump;
    satisfy torque by Unit::motor;
    satisfy load by Pump;
    satisfy load by Unit::pump;
}
";

/// The items of one cell of one row of the requirement table.
fn cell(t: &Value, row: &str, column: &str) -> Vec<Value> {
    for s in t["sections"].as_array().unwrap() {
        let at = s["columns"].as_array().unwrap().iter().position(|c| c["id"] == column).unwrap();
        if let Some(r) = s["rows"].as_array().unwrap().iter().find(|r| r["ref"] == row) {
            return r["cells"][at]["items"].as_array().unwrap().clone();
        }
    }
    panic!("no row {row}");
}

#[test]
fn b_the_coverage_report_splits_direct_and_implied() {
    let m = model("package P {\n    part def Pump;\n    requirement group {\n        requirement inner;\n    }\n    requirement own;\n    satisfy group by Pump;\n    satisfy own by Pump;\n}\n");
    let c = agent::trace::coverage_report(&m, None).unwrap();
    let rows = c["requirements"].as_array().unwrap();
    let row = |q: &str| rows.iter().find(|r| r["ref"] == q).unwrap_or_else(|| panic!("{c}"));
    assert!(c["basis_rule"].as_str().unwrap().contains("implied"));
    assert_eq!((row("P::own")["satisfied_basis"].as_str(), row("P::group")["satisfied_basis"].as_str()), (Some("direct"), Some("direct")), "{c}");
    assert_eq!(row("P::group::inner")["satisfied_basis"], "implied", "{c}");
    assert_eq!(row("P::group::inner")["satisfied_by"], json!([]), "an implied link does not make the requirement satisfied: {c}");
    let t = &c["totals"];
    assert_eq!((t["satisfied_direct"].as_u64(), t["satisfied_implied"].as_u64(), t["satisfied"].as_u64()), (Some(2), Some(1), Some(2)), "{t}");
}

#[test]
fn a_every_property_and_its_closure_is_listed_and_an_empty_one_is_present() {
    let m = model(BASE);
    let top = props(&m, "P::top");
    assert_eq!(top["properties"].as_array().unwrap().len(), 15);
    assert_eq!(refs(prop(&top, "Derived"), "items"), ["P::mid"]);
    assert_eq!(refs(prop(&top, "Derived"), "all_items"), ["P::mid", "P::leaf"]);
    assert_eq!(prop(&top, "Derived")["all_name"], "All Derived");
    assert_eq!(refs(prop(&top, "Refined By"), "items"), ["P::Gearbox"]);
    // The leaf: derived from mid, and by closure from top.
    let leaf = props(&m, "P::leaf");
    assert_eq!(refs(prop(&leaf, "Derived From"), "items"), ["P::mid"]);
    assert_eq!(refs(prop(&leaf, "Derived From"), "all_items"), ["P::mid", "P::top"]);
    // An empty property is present, with an empty list.
    let e = prop(&leaf, "Satisfied By");
    assert_eq!((e["count"].as_u64(), e["items"].as_array().map(|a| a.len()), e["all_items"].as_array().map(|a| a.len())), (Some(0), Some(0), Some(0)));
    assert_eq!(prop(&leaf, "Master")["items"], json!([]));
    // Satisfy, verify and allocate on both sides.
    let t = props(&m, "P::throughput");
    assert_eq!(refs(prop(&t, "Satisfied By"), "items"), ["P::Pump"]);
    assert_eq!(refs(prop(&t, "Verified By"), "items"), ["P::check"]);
    let pump = props(&m, "P::Pump");
    assert_eq!(refs(prop(&pump, "Satisfies"), "items"), ["P::throughput"]);
    assert_eq!(refs(prop(&pump, "Allocated To"), "items"), ["P::Gearbox"]);
    assert_eq!(refs(prop(&props(&m, "P::Gearbox"), "Allocated From"), "items"), ["P::Pump"]);
    assert_eq!(refs(prop(&props(&m, "P::check"), "Verifies"), "items"), ["P::throughput"]);
    assert_eq!(refs(prop(&props(&m, "P::Gearbox"), "Refines"), "items"), ["P::top"]);
    // Each entry has a file, a line and the link it came through.
    let it = &prop(&top, "Derived")["items"][0];
    assert_eq!((it["file"].as_str(), it["via"]["kind"].as_str(), it["basis"].as_str()), (Some("m.sysml"), Some("derive"), Some("direct")));
    assert!(it["line"].as_u64().unwrap() > 0 && it["via"]["line"].as_u64().unwrap() > 0, "{it}");
    // Each property is in a group.
    let table = trace_props::table();
    assert_eq!(table["properties"].as_array().unwrap().len(), 15);
}

#[test]
fn a_a_trace_to_a_requirement_counts_as_a_satisfy_link_and_says_so() {
    let m = model(BASE);
    let spare = props(&m, "P::spare");
    let sat = prop(&spare, "Satisfied By");
    assert_eq!(refs(sat, "items"), ["P::Gearbox"]);
    assert_eq!(sat["items"][0]["via"]["kind"], "trace");
    assert_eq!(refs(prop(&spare, "Traced To"), "items"), ["P::Gearbox"]);
    // A closure over a cycle ends.
    let cyc = model("package P { requirement a; requirement b;\n #SysMLv1::DeriveReqt dependency from a to b;\n #SysMLv1::DeriveReqt dependency from b to a; }");
    assert_eq!(refs(prop(&props(&cyc, "P::a"), "Derived"), "all_items"), ["P::b"]);
}

const WATCH: &str = "package P {
    part def Pump;
    part def Valve;
    requirement throughput { doc /* The pump moves 5 litres a minute. */ }
    requirement housing { doc /* The cabinet keeps water out. */ }
    satisfy throughput by Pump;
    satisfy housing by Valve;
}
";

fn listing(before: &str, now: &str, opts: &suspect::Opts, reviews: &suspect::Reviews) -> Value {
    suspect::list(&model(before), &model(now), opts, reviews)
}

fn changed_text() -> String {
    WATCH.replace("5 litres", "8 litres")
}

#[test]
fn c_an_unchanged_end_is_not_suspect_and_a_changed_text_is_with_before_and_after_and_the_link_line() {
    let none = listing(WATCH, WATCH, &Default::default(), &Default::default());
    assert_eq!((none["total"].as_u64(), none["links"].as_array().unwrap().len()), (Some(0), 0), "{none}");
    let l = listing(WATCH, &changed_text(), &Default::default(), &Default::default());
    assert_eq!((l["total"].as_u64(), l["suspect"].as_u64()), (Some(1), Some(1)), "{l}");
    let row = &l["links"][0];
    assert_eq!(row["key"], "satisfy|P::Pump|P::throughput");
    let ch = &row["changed"][0];
    assert_eq!((ch["field"].as_str(), ch["end"].as_str()), (Some("text"), Some("requirement")), "{row}");
    assert!(ch["before"].as_str().unwrap().contains("5 litres") && ch["after"].as_str().unwrap().contains("8 litres"), "{ch}");
    assert!(row["link"]["line"].as_u64().unwrap() > 0 && row["link"]["file"] == "m.sysml", "{row}");
    assert_eq!(row["cleared"], false);
    // The link of the other requirement is not listed.
    assert!(l["links"].as_array().unwrap().iter().all(|r| r["supplier"]["ref"] != "P::housing"), "{l}");
}

/// The review a person's clearing stores for the link `key` of `text`.
fn stamp(text: &str, key: &str, opts: &suspect::Opts) -> Value {
    let mut st = suspect::review_stamp(&model(text), key, opts).unwrap();
    // A review also holds the side it was made against: the older text's fingerprint as the listing showed it.
    let shown = listing(WATCH, text, opts, &Default::default());
    st["before_fingerprint"] = shown["links"].as_array().into_iter().flatten().find(|r| r["key"] == key).map(|r| r["fingerprint"]["before"].clone()).unwrap_or(json!("none"));
    st
}

#[test]
fn c_a_review_clears_only_the_values_it_names_and_the_next_change_makes_the_link_suspect_again() {
    let l = listing(WATCH, &changed_text(), &Default::default(), &Default::default());
    let key = l["links"][0]["key"].as_str().unwrap().to_string();
    let mut reviews = suspect::Reviews::new();
    reviews.entry(key.clone()).or_default().push(stamp(&changed_text(), &key, &Default::default()));
    let cleared = listing(WATCH, &changed_text(), &Default::default(), &reviews);
    assert_eq!((cleared["cleared"].as_u64(), cleared["suspect"].as_u64()), (Some(1), Some(0)), "{cleared}");
    // The end changes again: the earlier review does not clear it, and the listing says which field moved since.
    let nine = WATCH.replace("5 litres", "9 litres");
    let again = listing(WATCH, &nine, &Default::default(), &reviews);
    assert_eq!((again["cleared"].as_u64(), again["suspect"].as_u64()), (Some(0), Some(1)), "{again}");
    assert_eq!((again["links"][0]["earlier_reviews"].clone(), again["links"][0]["changed_since_last_review"].clone()), (json!(1), json!(["requirement text"])), "{again}");
    // A review for another way of tracking does not clear this one.
    let mut other = suspect::Reviews::new();
    let by_other = suspect::Opts { track: suspect::Track::Other, ..Default::default() };
    other.entry(key.clone()).or_default().push(stamp(&changed_text(), &key, &by_other));
    assert_eq!(listing(WATCH, &changed_text(), &Default::default(), &other)["suspect"], 1);
    // A record with the right fingerprint and wrong field hashes does not clear it.
    let mut forged = stamp(&changed_text(), &key, &Default::default());
    forged["field_hashes"]["requirement"]["text"] = json!("0".repeat(64));
    let mut f = suspect::Reviews::new();
    f.entry(key).or_default().push(forged);
    assert_eq!(listing(WATCH, &changed_text(), &Default::default(), &f)["suspect"], 1);
}

#[test]
fn c_sha256_matches_the_published_vectors() {
    assert_eq!(suspect::sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(suspect::sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(suspect::sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
}

#[test]
fn c_a_colliding_hash_does_not_hide_a_change_or_clear_a_link_and_no_value_can_imitate_a_field_boundary() {
    // A hash that is the same for everything: the change is still found (the values are compared), and a review made on another
    // text does not clear the link (the hash of each field is held as well).
    let always = |_: &[u8]| "same".to_string();
    let (b, n) = (model(WATCH), model(&changed_text()));
    let l = suspect::list_with(&b, &n, &Default::default(), &Default::default(), always);
    assert_eq!(l["total"], 1, "{l}");
    let key = l["links"][0]["key"].as_str().unwrap().to_string();
    let mut old_review = stamp(WATCH, &key, &Default::default());
    old_review["fingerprint"] = json!("same");
    let mut r = suspect::Reviews::new();
    r.entry(key).or_default().push(old_review);
    let l = suspect::list_with(&b, &n, &Default::default(), &r, always);
    assert_eq!((l["total"].as_u64(), l["cleared"].as_u64()), (Some(1), Some(0)), "{l}");
    // The old encoding joined fields with U+001F and U+001E: a value that holds them is the same as two fields there.
    let one = suspect::canonical(&[("requirement", vec![("name", "a".to_string()), ("text", "b".to_string())])]);
    let forged = suspect::canonical(&[("requirement", vec![("name", "a\u{1e}text\u{1f}b".to_string())])]);
    assert_ne!(suspect::sha256_hex(&one), suspect::sha256_hex(&forged));
    let split = suspect::canonical(&[("requirement", vec![("name", "ab".to_string())]), ("other", vec![("name", "c".to_string())])]);
    let moved = suspect::canonical(&[("requirement", vec![("name", "a".to_string())]), ("other", vec![("name", "bc".to_string())])]);
    assert_ne!(split, moved);
    // At the model: a requirement whose text imitates the separators is a change, not a match.
    let imitate = WATCH.replace("5 litres a minute.", "x\u{1e}values\u{1f}y");
    assert_eq!(listing(WATCH, &imitate, &Default::default(), &Default::default())["total"], 1);
}

const WATCH2: &str = "package P {
    part def Pump;
    package Reqs {
        requirement throughput { doc /* The pump moves 5 litres a minute. */ }
    }
    satisfy Reqs::throughput by Pump;
}
";

fn reasons(l: &Value) -> Vec<String> {
    l["links"].as_array().unwrap().iter().map(|x| x["reason"].as_str().unwrap().to_string()).collect()
}

#[test]
fn c_a_renamed_or_moved_or_recreated_end_is_never_skipped() {
    let now_text = |t: &str| t.replace("5 litres", "8 litres");
    // A rename together with a text change.
    let renamed = now_text(WATCH2).replace("throughput", "flowRate");
    let l = listing(WATCH2, &renamed, &Default::default(), &Default::default());
    assert!(l["total"].as_u64().unwrap() >= 1, "{l}");
    assert!(l["links"].as_array().unwrap().iter().any(|x| x["supplier"]["name"] == "flowRate" && x["reason"] != "link removed or its end removed"), "{l}");
    // A move to another package together with a text change.
    let moved = "package P {\n    part def Pump;\n    package Reqs {\n    }\n    package Other {\n        requirement throughput { doc /* The pump moves 8 litres a minute. */ }\n    }\n    satisfy Other::throughput by Pump;\n}\n";
    let l = listing(WATCH2, moved, &Default::default(), &Default::default());
    assert!(l["links"].as_array().unwrap().iter().any(|x| x["supplier"]["ref"] == "P::Other::throughput"), "moved end listed: {l}");
    // Delete and recreate with the same name and the same text: a new identity is not a match when the pairing says so; the
    // listing never says "nothing" for a link whose end was replaced with a changed text.
    let recreated = now_text(WATCH2);
    let l = listing(WATCH2, &recreated, &Default::default(), &Default::default());
    assert_eq!(l["total"], 1, "{l}");
    // A pure rename: the name is a tracked field, so it is suspect; without the name field it is not.
    let pure = WATCH2.replace("throughput", "flowRate");
    let l = listing(WATCH2, &pure, &Default::default(), &Default::default());
    assert!(l["total"].as_u64().unwrap() >= 1, "{l}");
    let no_name = suspect::Opts { fields: vec!["text", "values", "type"], ..Default::default() };
    let l = listing(WATCH2, &pure, &no_name, &Default::default());
    assert!(reasons(&l).iter().all(|r| r != "changed") || l["total"] == 0, "{l}");
}

#[test]
fn c_an_older_link_with_no_match_now_is_listed_and_the_reason_is_given() {
    let gone = WATCH.replace("    satisfy throughput by Pump;\n", "");
    let l = listing(WATCH, &gone, &Default::default(), &Default::default());
    assert!(reasons(&l).contains(&"link removed or its end removed".to_string()), "{l}");
    assert_eq!(l["by_reason"]["link_or_end_removed"], 1, "{l}");
    assert_eq!(l["links"][0]["cleared"], false);
    // A requirement the older revision did not have: its link is listed with the reason that names the older revision.
    let added = WATCH.replace("    satisfy housing by Valve;\n", "    requirement fresh { doc /* New. */ }\n    satisfy fresh by Valve;\n    satisfy housing by Valve;\n");
    let opts = suspect::Opts { since: "baseline \"B1\"".to_string(), ..Default::default() };
    let l = listing(WATCH, &added, &opts, &Default::default());
    assert!(reasons(&l).contains(&"end new, renamed or moved: not matched in baseline \"B1\"".to_string()), "{l}");
    assert!(l["not_checked"].as_array().unwrap().is_empty());
}

#[test]
fn c_the_tracked_end_and_the_fields_are_options() {
    let valve = WATCH.replace("part def Valve;", "part def Valve { attribute size = 3; }");
    let by_other = suspect::Opts { track: suspect::Track::Other, ..Default::default() };
    let l = listing(WATCH, &valve, &by_other, &Default::default());
    assert_eq!(l["total"], 1, "{l}");
    // The default end is the requirement: the same change is no change there.
    assert_eq!(listing(WATCH, &valve, &Default::default(), &Default::default())["total"], 0);
    // Only the fields named are compared.
    let name_only = suspect::Opts { fields: vec!["name"], ..Default::default() };
    assert_eq!(listing(WATCH, &changed_text(), &name_only, &Default::default())["total"], 0);
    // A Trace link between a requirement and another element is watched like a satisfy link.
    let tr = "package P { part def Pump; requirement r { doc /* a */ }\n #SysMLv1::Trace dependency from Pump to r; }";
    let l = listing(tr, &tr.replace("/* a */", "/* b */"), &Default::default(), &Default::default());
    assert_eq!((l["total"].as_u64(), l["links"][0]["link"]["read_as"].as_str()), (Some(1), Some("satisfy")), "{l}");
}

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn kinds(a: &Value) -> Vec<String> {
    a["check"]["findings"].as_array().unwrap().iter().map(|f| f["kind"].as_str().unwrap().to_string()).collect()
}

fn many(n: usize, doc: &str) -> String {
    let mut s = String::from("package P {\n    part def Pump;\n");
    for i in 0..n {
        s.push_str(&format!("    requirement r{i} {{ doc /* {doc} */ }}\n    satisfy r{i} by Pump;\n"));
    }
    s.push_str("}\n");
    s
}

/// A model with one statement that states more pairs than the reading bound allows, and one requirement the proposal edits.
fn bound_cut(doc: &str) -> String {
    let n = 101; // 10201 pairs: past graph::MAX_PAIRS_PER_STATEMENT
    let a: Vec<String> = (0..n).map(|i| format!("a{i}")).collect();
    let b: Vec<String> = (0..n).map(|i| format!("b{i}")).collect();
    let decl: String = a.iter().chain(&b).map(|x| format!("    part {x};\n")).collect();
    format!("package P {{\n    part def Pump;\n{decl}    dependency from {} to {};\n    requirement r0 {{ doc /* {doc} */ }}\n    satisfy r0 by Pump;\n}}\n", a.join(", "), b.join(", "))
}
