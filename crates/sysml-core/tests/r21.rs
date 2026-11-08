//! Tools round 21: requirements from a spreadsheet, the mass budget, and the two escapes of what the tools hand to a
//! person (a CSV cell that would run as a formula, model text inside a Markdown document).
//!
//! The import tests run the planned edits through the batch engine, so what is checked is the model text an import
//! leaves, not the plan.

#![allow(dead_code, unused_imports)]

mod common;

use serde_json::{json, Value};
use std::collections::BTreeMap;
use sysml_core::agent::{self, budget, intent, package, reqimport, rules, text, trace};
use sysml_core::api::{self, FileIn};
use sysml_core::Model;

fn files(text: &str) -> Vec<FileIn> {
    vec![FileIn { path: "m.sysml".into(), text: text.into() }]
}

fn model_of(files: &[FileIn]) -> Model {
    let mut m = api::to_model(files.to_vec());
    agent::add_namespace_checks(&mut m);
    m
}

fn model(text: &str) -> Model {
    let m = model_of(&files(text));
    assert!(!m.has_errors(), "the fixture must parse: {:?}", m.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>());
    m
}

/// Plan an import on `before` and run its edits; the report, the files after, and the model after.
fn import(before: &[FileIn], args: Value) -> (Value, Vec<FileIn>, Model) {
    let plan = reqimport::plan(&model_of(before), &args).unwrap_or_else(|f| panic!("the plan failed: {}", f.message));
    let after = if plan.edits.is_empty() { before.to_vec() } else { intent::apply_batch(before, &plan.edits).unwrap_or_else(|(i, f)| panic!("edit #{} failed: {}", i + 1, f.message)).files };
    let m = model_of(&after);
    assert!(!m.has_errors(), "the import left errors: {:?}\n{}", m.diagnostics.iter().map(|d| d.message.clone()).collect::<Vec<_>>(), after[0].text);
    (plan.report, after, m)
}

const SHEET: &str = "Id,Name,Text,Parent,Kind\r\n\
R-1,detectLanding,\"The station shall detect a landing, and clamp the \"\"drone\"\".\",,functional\r\n\
R-1.1,,The clamp shall hold 50 N.,R-1,Performance requirement\r\n\
R-2,stationMass,\"The station shall weigh\nat most 40 kg.\",,physical\r\n\
R-3,looks,The station shall be grey.,R-9,design constraint\r\n";

// ---- the csv reader ----------------------------------------------------------------------------------------------------

#[test]
fn the_csv_reader_reads_quoted_commas_quotes_and_line_breaks() {
    let rows = reqimport::parse_csv("\u{feff}a,b,c\r\n1,\"x, y\",\"say \"\"hi\"\"\"\n\n2,\"two\nlines\",\r\n3,,last").unwrap();
    assert_eq!(rows, vec![vec!["a", "b", "c"], vec!["1", "x, y", "say \"hi\""], vec!["2", "two\nlines", ""], vec!["3", "", "last"]]);
    assert!(reqimport::parse_csv("a,b\n1,\"never closed\n").unwrap_err().contains("never closed"));
    assert!(reqimport::parse_csv("a,b\n1,x\"y\n").unwrap_err().contains("line 2"));
    assert!(reqimport::parse_csv("a\n\"x\"y\n").unwrap_err().contains("after the closing quote"));
    assert_eq!(reqimport::parse_csv("").unwrap().len(), 0);
}

#[test]
fn a_kind_cell_is_read_as_the_mark_of_its_kind() {
    for (cell, mark) in [("functional", "functionalRequirement"), ("Functional Requirement", "functionalRequirement"), ("performanceRequirement", "performanceRequirement"), ("design constraint", "designConstraint"), ("Design", "designConstraint"), ("INTERFACE", "interfaceRequirement"), ("physical", "physicalRequirement"), ("business", "businessRequirement"), ("extended", "extendedRequirement")] {
        assert_eq!(reqimport::mark_of(cell), Some(mark), "{cell}");
    }
    assert_eq!(reqimport::mark_of("weird"), None);
    assert_eq!(reqimport::name_from_id("R-4.1"), "R_4_1");
    assert_eq!(reqimport::name_from_id("4.1"), "R_4_1");
    assert_eq!(reqimport::name_from_id("--"), "requirement");
}

// ---- the import --------------------------------------------------------------------------------------------------------

#[test]
fn a_row_that_exists_is_reported_and_not_overwritten_unless_update_is_given() {
    let (_, first, _) = import(&[], json!({ "csv": SHEET, "owner": "Station" }));
    let again = SHEET.replace("The station shall be grey.", "The station shall be light grey.");
    // The same file again: everything exists, one text differs, nothing is planned.
    let plan = reqimport::plan(&model_of(&first), &json!({ "csv": again, "owner": "Station" })).unwrap();
    assert!(plan.edits.is_empty(), "{:?}", plan.edits);
    assert_eq!(plan.report["counts"], json!({ "created": 0, "existing": 3, "changed": 1, "skipped": 0, "links": 0, "links_skipped": 0 }));
    let c = &plan.report["changed"][0];
    assert_eq!((c["id"].as_str(), c["row"].as_u64(), c["before"].as_str(), c["after"].as_str(), c["updated"].as_bool()), (Some("R-3"), Some(5), Some("The station shall be grey."), Some("The station shall be light grey."), Some(false)));
    // A text with a line break and quotes is the same text after the round trip: not reported as changed.
    assert!(plan.report["existing"].as_array().unwrap().iter().any(|x| x["id"] == "R-2"));
    // With update: one update_element, and the model takes the text of the file.
    let (r, after, m) = import(&first, json!({ "csv": again, "owner": "Station", "update": true }));
    assert_eq!(r["changed"][0]["updated"], true);
    assert_eq!(m.find("Station::looks").unwrap().doc.as_deref().map(str::trim), Some("The station shall be light grey."));
    assert_eq!(after[0].text.matches("requirement <").count(), 4);
    // A new row beside existing ones, derived from one that exists already.
    let more = format!("{SHEET}R-2.1,,The frame shall weigh at most 10 kg.,R-2,physical\r\n");
    let (r, _, m) = import(&first, json!({ "csv": more, "owner": "Station" }));
    assert_eq!((r["counts"]["created"].as_u64(), r["counts"]["existing"].as_u64(), r["counts"]["links"].as_u64()), (Some(1), Some(4), Some(1)));
    assert_eq!(r["links"][0]["target"], "Station::stationMass");
    assert!(m.find("Station::R_2_1").is_some());
}

#[test]
fn rows_that_cannot_be_imported_are_skipped_with_the_reason() {
    let csv = "id,name,text,kind\n,a,no id,\nR-1,one,first,\nR-1,two,the id again,\nR-2,one,the name again,\nR-3,three,odd kind,urgent\nR-4,four,fine,functional\n";
    let (r, _, m) = import(&[], json!({ "csv": csv, "owner": "P" }));
    assert_eq!((r["counts"]["created"].as_u64(), r["counts"]["skipped"].as_u64()), (Some(2), Some(4)));
    let why: Vec<(u64, String)> = r["skipped"].as_array().unwrap().iter().map(|x| (x["row"].as_u64().unwrap(), x["reason"].as_str().unwrap().to_string())).collect();
    assert_eq!(why[0].0, 2);
    assert!(why[0].1.contains("no id"));
    assert!(why[1].0 == 4 && why[1].1.contains("repeats the id of row 3"), "{why:?}");
    assert!(why[2].0 == 5 && why[2].1.contains("name \"one\" is taken"), "{why:?}");
    assert!(why[3].0 == 6 && why[3].1.contains("\"urgent\" is not a requirement kind"), "{why:?}");
    assert!(m.find("P::one").is_some() && m.find("P::four").is_some() && m.find("P::three").is_none());
}

#[test]
fn the_columns_are_found_by_header_or_named_and_a_missing_one_is_refused() {
    let csv = "Req No;x\n";
    assert!(reqimport::plan(&Model::default(), &json!({ "csv": csv, "owner": "P" })).err().unwrap().message.contains("no column for the id"));
    let csv = "Number,Title,Description,Type\n7,alpha,The unit shall run.,functional\n";
    let (r, _, m) = import(&[], json!({ "csv": csv, "owner": "P" }));
    assert_eq!(r["columns"], json!({ "id": "Number", "text": "Description", "name": "Title", "parent": null, "kind": "Type" }));
    assert_eq!(m.find("P::alpha").unwrap().short_name.as_deref(), Some("7"));
    let csv = "Key,Shall\nK-1,The unit shall stop.\n";
    let (r, _, _) = import(&[], json!({ "csv": csv, "owner": "P", "id_column": "key", "text_column": "SHALL" }));
    assert_eq!(r["created"][0]["id"], "K-1");
    let e = reqimport::plan(&Model::default(), &json!({ "csv": csv, "owner": "P", "id_column": "nope" })).err().unwrap();
    assert!(e.message.contains("no column \"nope\"") && e.message.contains("\"Key\", \"Shall\""), "{}", e.message);
    assert!(reqimport::plan(&Model::default(), &json!({ "csv": "", "owner": "P" })).err().unwrap().message.contains("empty"));
    assert!(reqimport::plan(&Model::default(), &json!({ "owner": "P" })).err().unwrap().message.contains("'csv'"));
    assert!(reqimport::plan(&Model::default(), &json!({ "csv": "id,text\n" })).err().unwrap().message.contains("'owner'"));
    assert!(reqimport::plan(&Model::default(), &json!({ "csv": "id,text\n1,x\n", "owner": "A::B" })).err().unwrap().message.contains("no element named"));
}

#[test]
fn the_row_and_size_limits_are_checked_before_any_work() {
    let mut csv = String::from("id,text\n");
    for i in 0..=reqimport::MAX_ROWS {
        csv.push_str(&format!("R-{i},t\n"));
    }
    let e = reqimport::plan(&Model::default(), &json!({ "csv": csv, "owner": "P" })).err().unwrap();
    assert!(e.message.contains("5001 rows") && e.message.contains("at most 5000"), "{}", e.message);
    let big = format!("id,text\nR-1,{}\n", "x".repeat(reqimport::MAX_BYTES));
    let e = reqimport::plan(&Model::default(), &json!({ "csv": big, "owner": "P" })).err().unwrap();
    assert!(e.message.contains("at most 2097152"), "{}", e.message);
}

// ---- what the tools hand to a person -----------------------------------------------------------------------------------

#[test]
fn a_csv_cell_that_would_run_as_a_formula_is_made_text() {
    for bad in ["=1+1", "+1", "-1", "@SUM(A1)", "\tx", "\rx"] {
        let cell = text::csv_field(bad);
        assert!(cell.starts_with('\'') || cell.starts_with("\"'"), "{bad:?} gave {cell:?}");
    }
    assert_eq!(text::csv_field("=a,b"), "\"'=a,b\"");
    assert_eq!(text::csv_field("a=b"), "a=b");
    assert_eq!(text::csv_field("R-1"), "R-1");
    assert_eq!(text::csv_field(""), "");
    // Every csv writer goes through it: the budget too. A value that starts with a minus is such a cell.
    let m = model("package P {\n    private import SI::*;\n    part def S {\n        part a : A;\n    }\n    part def A {\n        attribute mass = -2 [kg];\n    }\n}\n");
    let b = budget::budget(&m, None, None, None).unwrap();
    assert!(b["csv"].as_str().unwrap().contains(",'-2 [kg],not readable,"), "{}", b["csv"]);
    assert!(!b["csv"].as_str().unwrap().lines().any(|l| l.split(',').any(|c| c.starts_with(['=', '-', '+', '@']))), "{}", b["csv"]);
}

#[test]
fn model_text_cannot_break_a_markdown_table_or_inject_markup() {
    assert_eq!(text::md_text("a | b"), "a \\| b");
    assert_eq!(text::md_text("line one\nline two\r\n| x |"), "line one line two \\| x \\|");
    assert_eq!(text::md_text("<script>alert(1)</script>"), "\\<script\\>alert(1)\\</script\\>");
    assert_eq!(text::md_text("[click](http://x)"), "\\[click\\](http://x)");
    assert_eq!(text::md_text("`code` and *bold* and a\\b"), "\\`code\\` and \\*bold\\* and a\\\\b");
    // An underscore inside a word is plain text and stays; at the edge of a word it could start emphasis.
    assert_eq!(text::md_text("pad_sensor _x_ __y"), "pad_sensor \\_x\\_ \\_\\_y");
    assert_eq!(text::md_text("# heading"), "\\# heading");
    assert_eq!(text::md_cell("  "), "-");
    let t = text::md_table(&["A", "B"], &[vec!["x | y\n| z".into(), "ok".into()]]);
    assert_eq!(t.lines().count(), 3, "{t}");
    assert_eq!(t.lines().nth(2).unwrap().matches(" | ").count(), 1, "{t}");

    // The change package: a requirement text and a title that carry table and heading markup stay in their cell and line.
    let before = "package P {\n    requirement <'R-1'> r {\n        doc /* plain */\n    }\n}\n";
    let after = "package P {\n    requirement <'R-1'> r {\n        doc /* a | b <b>x</b> [l](u) `c`\n## not a heading */\n    }\n}\n";
    let meta = json!({ "title": "CR | 1\n## Injected", "reason": "because | so\n\n| a | b |", "author": "me", "from": { "revision": "a", "is": "current" }, "to": { "revision": "b", "is": "current" } });
    let p = package::change_package(&model(before), &model(after), &BTreeMap::new(), &meta);
    let md = p["markdown"].as_str().unwrap();
    assert!(md.starts_with("# CR \\| 1 \\#\\# Injected\n") || md.starts_with("# CR \\| 1 ## Injected\n"), "{}", md.lines().next().unwrap());
    assert!(!md.lines().any(|l| l.starts_with("## Injected") || l.starts_with("## not a heading")), "{md}");
    // The reason keeps its lines (round 27); every line is escaped, so the table row stays one row.
    assert!(md.contains("> \u{BB} because \\| so\n> \u{BB}\n> \u{BB} \\| a \\| b \\|\n"), "{md}");
    assert!(md.contains("| Reason | the proposer's words, quoted below the table |"), "{md}");
    assert!(md.contains("a \\| b \\<b\\>x\\</b\\> \\[l\\](u) \\`c\\` ## not a heading"), "{md}");
    // Every table row has the cells of its header: no cell was split.
    let lines: Vec<&str> = md.lines().collect();
    for (i, l) in lines.iter().enumerate() {
        if l.starts_with("|---") {
            let cols = l.matches("---").count();
            let mut j = i + 1;
            while j < lines.len() && lines[j].starts_with('|') {
                let cells = lines[j].replace("\\|", "").matches('|').count() - 1;
                assert_eq!(cells, cols, "row {j} of the record has {cells} cells under {cols} headers: {}", lines[j]);
                j += 1;
            }
        }
    }
}

// ---- the budget --------------------------------------------------------------------------------------------------------

const STATION: &str = "package P {
    private import SI::*;
    requirement <'R-7'> stationMass {
        doc /* The station shall have a mass of at most 40 kg. */
        attribute maxMass = 40 [kg];
    }
    part def Station {
        part arm : Arm;
        part sensor : Sensor [2];
        part rack : Rack;
        part cover : Cover;
        part spare : Sensor [0..*];
    }
    part def Arm {
        attribute mass = 12 [kg];
    }
    part def Sensor {
        attribute mass = 500 [g];
    }
    part def Rack {
        attribute mass = 4 [kg];
        part cell : Cell [3];
    }
    part def Cell {
        attribute mass : ISQ::MassValue = 2;
    }
    part def Cover;
}
";

fn row<'a>(b: &'a Value, name: &str) -> &'a Value {
    b["tree"].as_array().unwrap().iter().find(|r| r["name"] == name).unwrap_or_else(|| panic!("no row {name} in {}", b["text"]))
}

#[test]
fn the_budget_rolls_mass_up_with_units_and_multiplicity() {
    let m = model(STATION);
    let b = budget::budget(&m, None, None, None).unwrap();
    // arm 12 + sensor 2 x 0.5 + rack (4 + 3 x 2) + cover (not stated) + spare (0..* is no number: once, 0.5) = 23.5
    assert_eq!(b["total"].as_f64(), Some(23.5), "{}", b["text"]);
    assert_eq!(b["total_text"], "23.5 kg");
    assert_eq!((b["quantity"].as_str(), b["unit"].as_str()), (Some("mass"), Some("kg")));
    assert_eq!(b["roots"].as_array().unwrap().len(), 1);
    assert_eq!(b["roots"][0]["ref"], "P::Station");
    let (arm, sensor, rack, cell, cover, spare, top) = (row(&b, "arm"), row(&b, "sensor"), row(&b, "rack"), row(&b, "cell"), row(&b, "cover"), row(&b, "spare"), &b["tree"][0]);
    assert_eq!((arm["own"].as_f64(), arm["total"].as_f64(), arm["state"].as_str(), arm["stated_as"].as_str()), (Some(12.0), Some(12.0), Some("stated"), Some("12 [kg]")));
    // grams are converted, and the multiplicity multiplies.
    assert_eq!((sensor["own"].as_f64(), sensor["count"].as_f64(), sensor["rolled_up"].as_f64(), sensor["total"].as_f64()), (Some(0.5), Some(2.0), Some(0.5), Some(1.0)));
    // A part's total is its own mass plus what it holds; a value without a unit takes the unit of its quantity type.
    assert_eq!((cell["own"].as_f64(), cell["total"].as_f64(), cell["depth"].as_u64()), (Some(2.0), Some(6.0), Some(2)));
    assert_eq!((rack["own"].as_f64(), rack["rolled_up"].as_f64(), rack["parts"].as_u64()), (Some(4.0), Some(10.0), Some(1)));
    assert_eq!(cell["parent"], rack["index"]);
    // A part with no mass is "not stated", never zero in silence.
    assert_eq!((cover["state"].as_str(), cover["own"].is_null()), (Some("not stated"), true));
    assert_eq!(b["not_stated"].as_array().unwrap().len(), 1);
    assert_eq!(b["not_stated"][0]["ref"], "P::Station::cover");
    assert_eq!(b["complete"], false);
    assert!(b["says"].as_str().unwrap().contains("1 part(s) state no mass: the total is a lower bound"), "{}", b["says"]);
    // A multiplicity that is no number counts once, and the row says so.
    assert_eq!(spare["count"].as_f64(), Some(1.0));
    assert!(spare["notes"][0].as_str().unwrap().contains("0..* is not one number"), "{}", spare["notes"]);
    // The top is the sum of its parts.
    assert_eq!((top["state"].as_str(), top["total"].as_f64(), top["depth"].as_u64()), (Some("sum of its parts"), Some(23.5), Some(0)));
    // The limit, the margin and the verdict.
    let l = &b["limit"];
    assert_eq!((l["id"].as_str(), l["limit"].as_f64(), l["margin"].as_f64(), l["verdict"].as_str(), l["attribute"].as_str()), (Some("R-7"), Some(40.0), Some(16.5), Some("pass"), Some("P::stationMass::maxMass")));
    assert_eq!(b["verdict"], "pass");
    // The text is an indented tree; the csv has one line for each node and the totals.
    let text = b["text"].as_str().unwrap();
    assert!(text.contains("\nStation: the sum of its parts, total 23.5 kg\n  arm : Arm: own 12 kg\n  sensor : Sensor x2: own 0.5 kg, total 1 kg\n  rack : Rack: own 4 kg, total 10 kg\n    cell : Cell x3: own 2 kg, total 6 kg\n  cover : Cover: NOT STATED\n"), "{text}");
    let csv = b["csv"].as_str().unwrap();
    assert!(csv.starts_with("Level,Part,Definition,Count,Own mass (kg),Rolled-up mass of one (kg),Total with count (kg),Stated as,State,Notes\r\n"), "{csv}");
    assert!(csv.contains("\r\n2,    cell,Cell,3,2,2,6,2,stated,\r\n"), "{csv}");
    assert!(csv.contains("\r\n,Total,,,,,23.5,,1 part(s) not stated,\r\n,Limit (R-7 stationMass),,,,,40,,,\r\n,Margin,,,,,16.5,,pass,\r\n"), "{csv}");
}

#[test]
fn the_budget_takes_a_root_a_named_limit_and_refuses_what_it_cannot_read() {
    let m = model(STATION);
    // A part usage as the root: its own tree only.
    let b = budget::budget(&m, Some("mass"), Some("P::Station::rack"), Some("R-7")).unwrap();
    assert_eq!((b["total"].as_f64(), b["tree"].as_array().unwrap().len(), b["limit"]["margin"].as_f64()), (Some(10.0), 2, Some(30.0)));
    assert_eq!(budget::budget(&m, None, None, Some("P::stationMass")).unwrap()["limit"]["id"], "R-7");
    assert!(budget::budget(&m, Some("power"), None, None).err().unwrap().message.contains("quantity \"power\" is not built"));
    assert!(budget::budget(&m, None, Some("P::stationMass"), None).err().unwrap().message.contains("the root of a budget is a part"));
    assert!(budget::budget(&m, None, None, Some("R-99")).is_err());
    // A requirement without a limit, named as the limit.
    let m2 = model(&STATION.replace("        attribute maxMass = 40 [kg];\n", ""));
    assert!(budget::budget(&m2, None, None, Some("R-7")).err().unwrap().message.contains("states no mass limit"));
    let b2 = budget::budget(&m2, None, None, None).unwrap();
    assert_eq!((b2["verdict"].as_str(), b2["limit"].is_null()), (Some("no limit"), true));
    // A value that is no amount of mass is not counted and not hidden.
    let m3 = model(&STATION.replace("attribute mass = 12 [kg];", "attribute mass = 12 [m];"));
    let b3 = budget::budget(&m3, None, None, None).unwrap();
    assert_eq!(row(&b3, "arm")["state"], "not readable");
    assert!(row(&b3, "arm")["notes"][0].as_str().unwrap().contains("unit \"m\" is not a unit of mass"), "{}", row(&b3, "arm")["notes"]);
    assert_eq!(b3["total"].as_f64(), Some(11.5));
    assert_eq!(b3["not_stated"].as_array().unwrap().len(), 2);
    // A usage that states its own mass overrides its definition; tonnes are converted.
    let m4 = model(&STATION.replace("part arm : Arm;", "part arm : Arm {\n            attribute mass = 0.02 [t];\n        }"));
    assert_eq!(row(&budget::budget(&m4, None, None, None).unwrap(), "arm")["own"].as_f64(), Some(20.0));
    // A definition that holds itself ends.
    let m5 = model("package P {\n    private import SI::*;\n    part def A {\n        attribute mass = 1 [kg];\n        part again : A;\n    }\n}\n");
    assert_eq!(budget::budget(&m5, None, Some("P::A"), None).unwrap()["total"].as_f64(), Some(2.0));
    // No part at all.
    let empty = budget::budget(&model("package P {\n}\n"), None, None, None).unwrap();
    assert_eq!((empty["total"].as_f64(), empty["tree"].as_array().unwrap().len()), (Some(0.0), 0));
}

#[test]
fn the_change_package_has_a_budget_section_with_before_and_after() {
    let before = model(STATION);
    let after = model(&STATION.replace("part sensor : Sensor [2];", "part sensor : Sensor [2];\n        part arm2 : Arm;").replace("attribute mass = 12 [kg];", "attribute mass = 15 [kg];"));
    let meta = json!({ "title": "CR-9", "from": { "revision": "a", "is": "current" }, "to": { "revision": "b", "is": "current" } });
    let p = package::change_package(&before, &after, &BTreeMap::new(), &meta);
    let b = &p["budget"];
    assert_eq!((b["quantity"].as_str(), b["relevant"].as_bool()), (Some("mass"), Some(true)));
    assert_eq!((b["before"]["total"].as_f64(), b["after"]["total"].as_f64()), (Some(23.5), Some(41.5)));
    assert_eq!((b["before"]["verdict"].as_str(), b["after"]["verdict"].as_str()), (Some("pass"), Some("fail")));
    assert_eq!(b["after"]["limit"]["margin"].as_f64(), Some(-1.5));
    let rows: Vec<(&str, &str, &str, &str)> = b["rows"].as_array().unwrap().iter().map(|r| (r["change"].as_str().unwrap(), r["label"].as_str().unwrap(), r["before"].as_str().unwrap(), r["after"].as_str().unwrap())).collect();
    assert!(rows.contains(&("changed", "Station", "total 23.5 kg", "total 41.5 kg")), "{rows:?}");
    assert!(rows.contains(&("changed", "Station::arm", "12 kg", "15 kg")), "{rows:?}");
    assert!(rows.contains(&("added", "Station::arm2", "", "15 kg")), "{rows:?}");
    let md = p["markdown"].as_str().unwrap();
    for part in ["## 4. Budget: mass", "| Total mass | 23.5 kg | 41.5 kg |", "| Limit | 40 kg (R-7 stationMass) | 40 kg (R-7 stationMass) |", "| Margin | 16.5 kg | -1.5 kg |", "| Verdict | PASS | FAIL |", "| added | Station::arm2 | - | 15 kg |"] {
        assert!(md.contains(part), "the record lacks: {part}\n{md}");
    }
    // A model that states no mass and no limit has no budget section.
    let plain = model("package P {\n    part def A;\n}\n");
    let q = package::change_package(&plain, &plain, &BTreeMap::new(), &meta);
    assert_eq!(q["budget"]["relevant"], false);
    assert!(!q["markdown"].as_str().unwrap().contains("Budget"));
}
