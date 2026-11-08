//! Native cost of one write on a big model. Usage: edit_timing MODEL_DIR EDITS_JSON
use std::{path::Path, time::Instant};
use sysml_core::{api, agent};

fn files(dir: &Path, out: &mut Vec<api::FileIn>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.extension().is_some_and(|e| e == "sysml") {
            out.push(api::FileIn { path: path.to_string_lossy().into_owned(), text: std::fs::read_to_string(&path).unwrap() });
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut input = Vec::new();
    files(Path::new(&a[1]), &mut input);
    let edits: Vec<serde_json::Value> = serde_json::from_str(&std::fs::read_to_string(&a[2]).unwrap()).unwrap();
    let t = Instant::now();
    let m = api::to_model(input.clone());
    println!("to_model {:.0} ms ({} elements)", t.elapsed().as_secs_f64() * 1e3, m.elements.len());
    let t = Instant::now();
    let m2 = agent::tools::model_of_files(input.clone());
    println!("model_of_files {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    drop((m, m2));
    let only: Option<usize> = a.get(3).and_then(|x| x.parse().ok());
    for n in only.map(|n| vec![n]).unwrap_or(vec![1usize, edits.len()]) {
        let before = agent::intent::analyses_run();
        let t = Instant::now();
        let r = agent::intent::apply_batch(&input, &edits[..n]);
        println!("apply_batch {} edits: {:.0} ms, analyses {}, ok {}", n, t.elapsed().as_secs_f64() * 1e3, agent::intent::analyses_run() - before, r.is_ok());
        if let Err((i, f)) = r { println!("  failed at {i}: {}", f.message); }
    }
}
