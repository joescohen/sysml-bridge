//! Round 37: the one formula guard of every CSV the tools write.
use sysml_core::agent::text::{csv_all_quoted, csv_field, formula_safe, strip_formula_guard};

#[test]
fn a_field_that_starts_a_formula_is_guarded_after_white_space_and_in_full_width_forms() {
    for bad in ["=1+1", "+1", "-1", "@SUM(A1)", "  =1", "\t=1", " \u{a0}+1", "\u{ff1d}1", "\u{ff0b}1", "\u{ff0d}1", "\u{ff20}A", "\u{200b}=1", "\u{2212}1", "\r=1"] {
        assert!(formula_safe(bad).starts_with('\''), "{bad:?} is guarded");
        assert_eq!(strip_formula_guard(&formula_safe(bad)), bad, "{bad:?} reads back as written");
    }
    for fine in ["", "plain", "a=b", "x-1", "'quoted", "1.5", " text"] {
        assert_eq!(formula_safe(fine), fine, "{fine:?} is not touched");
        assert_eq!(strip_formula_guard(fine), fine);
    }
    // One writer: the plain and the all-quoted forms guard alike.
    assert_eq!(csv_field("  =cmd"), "'  =cmd");
    let all = csv_all_quoted(&["A".into(), "B".into()], &[vec!["\u{ff1d}1".into(), "ok, \"fine\"".into()]]);
    assert_eq!(all, "\"A\",\"B\"\r\n\"'\u{ff1d}1\",\"ok, \"\"fine\"\"\"\r\n");
}
