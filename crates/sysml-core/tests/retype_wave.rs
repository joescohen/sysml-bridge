//! A batch of plain retypes (and the flow statements a retype rewrites) gives the same files and replies whether it runs
//! one edit at a time or as one retype wave.
use serde_json::{json, Value};
use sysml_core::agent::{self, intent};
use sysml_core::api::FileIn;

const MODEL: &str = "package P {
    item def TrackList;
    item def BitResult;
    part def Rcs {
        action def Report {
            perform action run ::> report {
                out item trackList : P::TrackList;
            }
            out item trackList : P::TrackList;
            flow of P::TrackList from run.trackList to trackList;
        }
        action report {
            out item trackList : P::TrackList;
        }
    }
    part def Con {
        action show {
            in item trackList : P::TrackList;
        }
    }
}
";

// the analysis counter is shared by the whole process: the tests run one at a time
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn files() -> Vec<FileIn> {
    vec![FileIn { path: "p.sysml".into(), text: MODEL.into() }]
}

fn batch_with(on: bool, edits: &[Value]) -> Result<intent::Batch, (usize, agent::Fail)> {
    intent::set_waves(Some(on));
    let r = intent::apply_batch(&files(), edits);
    intent::set_waves(None);
    r
}

fn edits() -> Vec<Value> {
    vec![
        json!({"op": "update_element", "ref": "P::Rcs::report::trackList", "typed_by": "P::BitResult"}),
        json!({"op": "update_element", "ref": "P::Rcs::Report::trackList", "typed_by": "P::BitResult"}),
        json!({"op": "update_element", "ref": "P::Rcs::Report::run::trackList", "typed_by": "P::BitResult"}),
        json!({"op": "update_element", "ref": "P::Con::show::trackList", "typed_by": "P::BitResult"}),
    ]
}

#[test]
fn retypes_give_the_same_files_and_replies_in_one_wave() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let one = intent::analyses_run();
    let slow = batch_with(false, &edits()).expect("one by one");
    let slow_runs = intent::analyses_run() - one;
    let one = intent::analyses_run();
    let fast = batch_with(true, &edits()).expect("one wave");
    let runs = intent::analyses_run() - one;
    assert_eq!(fast.files[0].text, slow.files[0].text);
    assert_eq!(fast.results, slow.results);
    assert!(runs <= 4 && runs * 2 <= slow_runs, "{runs} analyses for one wave, {slow_runs} one by one");
    assert!(fast.files[0].text.contains("out item trackList : P::BitResult;"));
}

#[test]
fn a_retype_that_does_not_hold_runs_one_by_one_and_fails_the_same() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let mut e = edits();
    e.push(json!({"op": "update_element", "ref": "P::Rcs::report::trackList", "typed_by": "P::Nothing"}));
    let slow = batch_with(false, &e).err().expect("refused one by one");
    let fast = batch_with(true, &e).err().expect("refused in a wave");
    assert_eq!(slow.0, fast.0);
}
