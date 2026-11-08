//! Text edits and readers must never panic on multi-byte text, on a file that stops half way, or on a span
//! that was computed on other text. A panic here takes the whole server down, so every test runs the
//! operation under `catch_unwind` and reports the ones that panic, instead of stopping at the first.

mod common;

use std::panic::{catch_unwind, AssertUnwindSafe};
use sysml_core::api::{self, FileIn};
use sysml_core::edit::{self, EditOp};
use sysml_core::{Model, Span};

/// Values for every text argument: plain, two-byte, three-byte, four-byte, mixed, a quote, and empty.
const INPUTS: &[&str] = &["x", "\u{e9}", "\u{65e5}\u{672c}", "\u{1f600}", "a\u{e9}\u{65e5}\u{1f600}b", "\u{3000}", "o'k", ""];

const SOURCES: &[&str] = &[
    // Multi-byte names, values, docs and short names.
    "package P { part def A { attribute x : Real = 5; part y : A[2]; doc /* hello */ } }",
    "package 'P\u{e9}' { part def '\u{65e5}\u{672c}' { attribute '\u{e9}' : Real = 5; part 'y\u{1f600}' : '\u{65e5}\u{672c}'[2] :> '\u{65e5}\u{672c}'; doc /* \u{e9} \u{65e5}\u{672c} \u{1f600} */ } }",
    "package P { part def <'\u{e9}'> A; part def <'\u{65e5}\u{672c}'> B { part c :>> '\u{e9}'; } part def <R1> C :> A, B; }",
    // Multi-byte white space before `;`, before a body, and after a name.
    "package P { part def A { attribute x : Real\u{3000}; attribute y\u{a0}; part z : A\u{2003}{ } } }",
    "package P { part def A\u{3000}{\u{3000}} part b : A\u{a0}\u{a0}; attribute c = 1\u{3000}\u{3000}; }",
    // Files that stop half way, with the last token ending in a multi-byte character.
    "package P { part def A {",
    "package P {",
    "part def A {",
    "part def A { ",
    "part def '\u{e9}",
    "package P { part '\u{65e5}\u{672c}",
    "package '\u{1f600}",
    "package P { part def A { part x : '\u{e9}",
    "package P { attribute x = \"\u{e9}",
    "package P { attribute x = 'a\\\u{e9}",
    "package P { part def A { attribute x : Real = 5 \u{3000}",
    "package P { part a : A [ '\u{e9}' ]; part b[\u{e9}]; part c[1..*]; }",
    // Line ends and notes with multi-byte text next to the edited statement.
    "// \u{e9}\n/* \u{65e5}\u{672c} */ package P { part def A; /* \u{1f600} */ part x : A; }\r\n",
    "package P {\r\n    part def A;\r\n\u{3000}  part def B;\r\n}\r\n",
];

/// Every operation, with each input in each text argument.
fn ops_for(target: &str) -> Vec<EditOp> {
    let t = || target.to_string();
    let mut ops = vec![EditOp::Delete { target: t() }, EditOp::ClearType { target: t() }];
    for i in 0..3 {
        ops.push(EditOp::RemoveSpecialization { target: t(), index: i });
    }
    for s in INPUTS {
        let s = s.to_string();
        ops.push(EditOp::AddElement { owner: Some(t()), file: None, text: format!("part def {s};") });
        ops.push(EditOp::AddElement { owner: Some(t()), file: None, text: s.clone() });
        ops.push(EditOp::Rename { target: t(), new_name: s.clone() });
        ops.push(EditOp::Replace { target: t(), text: s.clone() });
        ops.push(EditOp::SetDoc { target: t(), doc: s.clone() });
        ops.push(EditOp::AddSpecialization { target: t(), general: s.clone() });
        ops.push(EditOp::SetType { target: t(), type_ref: s.clone() });
        ops.push(EditOp::SetMultiplicity { target: t(), multiplicity: s.clone() });
        ops.push(EditOp::SetValue { target: t(), value: s.clone() });
        ops.push(EditOp::SetShortName { target: t(), short: s.clone() });
        ops.push(EditOp::SetRedefines { target: t(), names: if s.is_empty() { vec![] } else { vec![s.clone(), s.clone()] } });
    }
    ops
}

fn describe(src: &str, op: &EditOp) -> String {
    format!("{op:?} on {src:?}")
}

/// Run `f`; `Some(message)` when it panicked.
fn panicked<T>(f: impl FnOnce() -> T) -> Option<String> {
    catch_unwind(AssertUnwindSafe(f)).err().map(|e| {
        e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into())
    })
}

fn model(text: &str) -> Model {
    api::to_model(vec![FileIn { path: "a.sysml".into(), text: text.into() }])
}

#[test]
fn parsing_never_panics_on_a_name_that_stops_inside_a_multibyte_character() {
    let mut bad = Vec::new();
    for src in SOURCES {
        // Cut the text at every byte that starts a character, and also keep the text whole.
        for (i, _) in src.char_indices().chain(std::iter::once((src.len(), ' '))) {
            let cut = &src[..i];
            if let Some(m) = panicked(|| {
                let m = model(cut);
                // The readers a client calls on every element.
                for e in &m.elements {
                    let _ = sysml_core::agent::element_details(&m, &e.qualified_name, None);
                    let _ = sysml_core::agent::export_sysml(&m, Some(&e.qualified_name));
                }
                let _ = api::analysis(&m);
                let _ = api::outline(&m, None, 6);
            }) {
                bad.push(format!("{cut:?}: {m}"));
            }
        }
    }
    assert!(bad.is_empty(), "{} panics, first: {:?}", bad.len(), &bad[..bad.len().min(5)]);
}

#[test]
fn a_name_in_quotes_that_is_cut_off_keeps_its_characters() {
    // The closing quote is missing, so the last character is part of the name, not a quote to strip.
    assert_eq!(sysml_core::lexer::unquote("'\u{e9}"), "\u{e9}");
    assert_eq!(sysml_core::lexer::unquote("'\u{65e5}\u{672c}"), "\u{65e5}\u{672c}");
    assert_eq!(sysml_core::lexer::unquote("\"\u{1f600}"), "\u{1f600}");
    assert_eq!(sysml_core::lexer::unquote("'ab'"), "ab");
    assert_eq!(sysml_core::lexer::unquote("'\u{e9}'"), "\u{e9}");
    assert_eq!(sysml_core::lexer::unquote("'"), "'");
    assert_eq!(sysml_core::lexer::unquote("''"), "");
    assert_eq!(sysml_core::lexer::unquote("'a\\'"), "a'");
    let m = model("package P { part def A { part x : '\u{e9}");
    assert!(m.elements.iter().any(|e| e.name.as_deref() == Some("P")));
}

#[test]
fn no_edit_operation_panics_on_multibyte_text() {
    let mut bad = Vec::new();
    let mut ran = 0;
    for src in SOURCES {
        let m = model(src);
        for e in &m.elements {
            for op in ops_for(&e.qualified_name) {
                ran += 1;
                match catch_unwind(AssertUnwindSafe(|| edit::apply(&m, &op))) {
                    Err(p) => {
                        let why = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                        bad.push(format!("{}: {why}", describe(src, &op)));
                    }
                    Ok(Ok(changes)) => {
                        // The result is read again by the next call; it must not panic either.
                        for c in changes {
                            if panicked(|| model(&c.text)).is_some() {
                                bad.push(format!("re-reading the result of {}", describe(src, &op)));
                            }
                        }
                    }
                    Ok(Err(_)) => {}
                }
            }
        }
        // And the add at the end of a file, with and without that file.
        for s in INPUTS {
            for file in [None, Some("a.sysml".to_string()), Some("new.sysml".to_string())] {
                let op = EditOp::AddElement { owner: None, file, text: format!("part def {s};") };
                ran += 1;
                if let Some(why) = panicked(|| edit::apply(&m, &op)) {
                    bad.push(format!("{}: {why}", describe(src, &op)));
                }
            }
        }
    }
    assert!(ran > 5000, "the harness ran only {ran} edits");
    assert!(bad.is_empty(), "{} panics in {ran} edits, first: {:#?}", bad.len(), &bad[..bad.len().min(6)]);
}

#[test]
fn a_value_after_a_multibyte_space_is_added_in_place() {
    // `head_end` walked back one byte per space character, which lands inside U+3000.
    let text = "package P { part def A { attribute x : Real\u{3000}; } }";
    let m = model(text);
    let out = edit::apply(&m, &EditOp::SetValue { target: "P::A::x".into(), value: "5".into() }).unwrap();
    assert!(out[0].text.contains("attribute x : Real = 5\u{3000};"), "{}", out[0].text);
}

#[test]
fn a_body_that_is_only_an_open_brace_takes_a_member() {
    // The body span is `{` alone, so `body.start + 1 > body.end - 1`.
    for text in ["part def A {", "part def A {\u{3000}", "package P { part def A {", "part def A { part '\u{e9}"] {
        let m = model(text);
        let target = if text.starts_with("package") { "P::A" } else { "A" };
        let r = catch_unwind(AssertUnwindSafe(|| edit::apply(&m, &EditOp::AddElement { owner: Some(target.into()), file: None, text: "part x;".into() })));
        assert!(r.is_ok(), "panic adding into {text:?}");
    }
}

#[test]
fn a_model_whose_text_changed_is_refused_not_cut_in_the_middle_of_a_character() {
    // The spans were computed on `old`; the file now holds `new`, whose bytes at those offsets are inside characters.
    let old = "package P { part def A { attribute x : Real = 1; part y : A; } }";
    let new = "package \u{65e5}\u{672c}\u{1f600}\u{65e5}\u{672c}\u{1f600}\u{65e5}\u{672c}\u{1f600} \u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}";
    let mut bad = Vec::new();
    for shift in 0..4 {
        let mut m = model(old);
        m.files[0].text = format!("{}{new}", "\u{e9}".repeat(shift));
        for e in m.elements.clone() {
            for op in ops_for(&e.qualified_name) {
                if let Some(why) = panicked(|| edit::apply(&m, &op)) {
                    bad.push(format!("{}: {why}", describe(&m.files[0].text, &op)));
                }
            }
        }
        for sp in [Span::new(0, 5), Span::new(3, 40), Span::new(7, 7), Span::new(100, 3)] {
            for off in [sp.start, sp.end, 1, 2, 3, 4, 5, 6, 7] {
                if let Some(why) = panicked(|| m.line_col(0, off)) {
                    bad.push(format!("line_col({off}): {why}"));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} panics, first: {:#?}", bad.len(), &bad[..bad.len().min(6)]);
}

#[test]
fn line_and_column_of_an_offset_inside_a_character_do_not_panic() {
    let text = "package \u{65e5}\u{672c} {\n  part def \u{1f600};\n}\n";
    let m = model(text);
    for off in 0..text.len() + 5 {
        let (line, col) = m.line_col(0, off);
        assert!(line >= 1 && col >= 1, "offset {off}");
    }
    // The answer for a boundary does not change.
    assert_eq!(m.line_col(0, text.find("part").unwrap()), (2, 3));
}

#[test]
fn a_rename_cannot_grow_the_file_without_a_limit() {
    // One name used many times; the new name is written once per use.
    let mut text = String::from("package P { part def A; ");
    for i in 0..20_000 {
        text.push_str(&format!("part p{i} : A; "));
    }
    text.push('}');
    let m = model(&text);
    // A name no model needs is refused before any text is built.
    let e = edit::apply(&m, &EditOp::Rename { target: "P::A".into(), new_name: "N".repeat(20_000) }).expect_err("a 20 000 byte name");
    assert!(e.contains("too long"), "{e}");
    // A name that is fine alone, written 20 000 times, makes more text than one edit may add.
    let e = edit::apply(&m, &EditOp::Rename { target: "P::A".into(), new_name: "N".repeat(900) }).expect_err("18 MB of new text");
    assert!(e.contains("larger than") || e.contains("too much"), "{e}");
    // A normal rename of the same name still works.
    let ok = edit::apply(&m, &EditOp::Rename { target: "P::A".into(), new_name: "B".into() }).unwrap();
    assert!(ok[0].text.contains("part p19999 : B;"));
}

#[test]
fn a_short_name_cannot_close_its_quote_and_add_a_statement() {
    let m = model("package P { part def A; }");
    let hostile = "x'> part def Injected; <'y";
    let out = edit::apply(&m, &EditOp::SetShortName { target: "P::A".into(), short: hostile.into() });
    match out {
        Err(_) => {}
        Ok(changes) => {
            let after = model(&changes[0].text);
            assert!(after.elements.iter().all(|e| e.name.as_deref() != Some("Injected")), "{}", changes[0].text);
            assert!(after.diagnostics.iter().all(|d| d.severity != sysml_core::Severity::Error), "{}", changes[0].text);
        }
    }
    // A line break would end the quoted name early.
    assert!(edit::apply(&m, &EditOp::SetShortName { target: "P::A".into(), short: "a\npart def Injected;\n".into() }).is_err());
    assert!(edit::apply(&m, &EditOp::Rename { target: "P::A".into(), new_name: "a\npart def Injected;\n".into() }).is_err());
    // A plain short name still works.
    let ok = edit::apply(&m, &EditOp::SetShortName { target: "P::A".into(), short: "R-1".into() }).unwrap();
    assert!(ok[0].text.contains("<'R-1'>"), "{}", ok[0].text);
}

// ---- the byte limits of the tool layer, with text whose characters are 2, 3 and 4 bytes wide ----

const TOOL_BASE: &str = "package Demo {\n    part def Vehicle {\n        attribute mass : Real = 5;\n        part engine : Engine;\n    }\n    part def Engine;\n    state def S {\n        state a;\n        state b;\n    }\n}\n";

fn tool_files() -> Vec<FileIn> {
    vec![FileIn { path: "demo.sysml".into(), text: TOOL_BASE.into() }]
}

/// Text of `bytes` bytes (or one byte more when the character is wider) made of one wide character.
fn wide(c: char, bytes: usize) -> String {
    c.to_string().repeat(bytes / c.len_utf8())
}

#[test]
fn every_byte_limit_of_the_tool_layer_holds_for_wide_characters() {
    use serde_json::json;
    use sysml_core::agent::intent;
    // The limits in force: multiplicity 40, short name 60, expression 200, source id 256, name 2000 (and 256 KiB, the old one-edit limit, still checked for wide characters).
    let limits = [40usize, 60, 200, 256, 2000, 256 << 10];
    let mut bad = Vec::new();
    let mut ran = 0;
    for limit in limits {
        for c in ['\u{e9}', '\u{65e5}', '\u{1f600}', 'x'] {
            for delta in [-5i64, -4, -3, -2, -1, 0, 1, 2, 3, 4, 5] {
                let n = (limit as i64 + delta) as usize;
                for text in [wide(c, n), format!("{}{}", "a".repeat(n.saturating_sub(c.len_utf8())), c), format!("{c}{}", "a".repeat(n.saturating_sub(c.len_utf8())))] {
                    let calls = vec![
                        json!({"op": "create_element", "kind": "part def", "name": text, "owner": "Demo"}),
                        json!({"op": "create_element", "kind": "attribute", "name": "q", "owner": "Demo::Vehicle", "value": text, "short_name": text, "multiplicity": text, "doc": text, "typed_by": text}),
                        json!({"op": "create_element", "kind": "part def", "name": "Z", "owner": "Demo", "source_id": text}),
                        json!({"op": "update_element", "ref": "Demo::Vehicle::mass", "value": text}),
                        json!({"op": "update_element", "ref": "Demo::Vehicle::mass", "short_name": text}),
                        json!({"op": "update_element", "ref": "Demo::Vehicle::engine", "multiplicity": text}),
                        json!({"op": "update_element", "ref": "Demo::Vehicle::engine", "rename": text}),
                        json!({"op": "update_element", "ref": "Demo::Vehicle::engine", "typed_by": text}),
                        json!({"op": "update_element", "ref": "Demo::Vehicle::engine", "redefines": [text]}),
                        json!({"op": "update_element", "ref": "Demo::Vehicle", "specializes": text, "doc": text}),
                        json!({"op": "update_element", "ref": "Demo::Vehicle::mass", "doc": text, "value": "1", "short_name": "R-1", "multiplicity": "2"}),
                        json!({"op": "rename", "target": "Demo::Engine", "new_name": text}),
                        json!({"op": "add_element", "owner": "Demo", "text": format!("part def {text};")}),
                        json!({"op": "replace", "target": "Demo::Engine", "text": text}),
                        json!({"op": "set_doc", "target": "Demo::Engine", "doc": text}),
                        json!({"op": "create_relationship", "kind": "transition", "source": "Demo::S::a", "target": "Demo::S::b", "trigger": format!("after {text}")}),
                        json!({"op": "create_relationship", "kind": "transition", "source": "Demo::S::a", "target": "Demo::S::b", "trigger": format!("when {text}"), "guard": text, "effect": text}),
                        json!({"op": "create_relationship", "kind": "dependency", "source": text, "target": "Demo::Engine"}),
                        json!({"op": "delete_element", "ref": text}),
                        json!({"op": "update_element", "ref": text, "rename": "Y"}),
                    ];
                    for call in calls {
                        ran += 1;
                        match catch_unwind(AssertUnwindSafe(|| intent::apply_op(&tool_files(), &call))) {
                            Err(p) => {
                                let why = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
                                bad.push(format!("panic: {why} for {}", &call.to_string().chars().take(120).collect::<String>()));
                            }
                            Ok(Err(f)) => {
                                // A refusal says what was wrong without repeating a request-sized text.
                                if f.message.len() > 4000 {
                                    bad.push(format!("a refusal of {} bytes for {}", f.message.len(), &call.to_string().chars().take(120).collect::<String>()));
                                }
                            }
                            Ok(Ok(applied)) => {
                                let after = api::to_model(applied.files);
                                // Raw text edits may write any text; the write gate refuses a syntax error. The others promise parsed text.
                                let raw = matches!(call["op"].as_str(), Some("add_element" | "replace" | "set_doc"));
                                if !raw && after.diagnostics.iter().any(|d| d.severity == sysml_core::Severity::Error) {
                                    // A call may be accepted only when what it wrote parses.
                                    bad.push(format!("accepted but wrote a syntax error: {}", &call.to_string().chars().take(120).collect::<String>()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(ran > 5000, "ran {ran}");
    assert!(bad.is_empty(), "{} problems in {ran} calls, first: {:#?}", bad.len(), &bad[..bad.len().min(8)]);
}
