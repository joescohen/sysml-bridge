//! The server as the place the shared read tools run (`sysml_core::agent::tools::Ctx`): the model from the files of the workspace,
//! and the stores the server keeps (write log, baselines, reviews).

use super::revs::{bar_line, pair, versions};
use super::{model_files, suspect, writes, ToolErr};
use crate::App;
use serde_json::{json, Value};
use sysml_core::agent::suspect::Reviews;
use sysml_core::agent::tools::{Ctx, Guard, SuspectPair};
use sysml_core::api::FileIn;
use sysml_core::Model;

impl Ctx for App {
    fn model(&self) -> std::sync::Arc<Model> {
        std::sync::Arc::new(super::model(self))
    }

    fn files(&self) -> Vec<FileIn> {
        model_files(self)
    }

    fn all_files(&self) -> Vec<FileIn> {
        self.store.read_all()
    }

    fn ids(&self) -> std::collections::BTreeMap<String, String> {
        super::ids::effective(self)
    }

    fn read_file(&self, path: &str) -> Result<String, String> {
        self.store.read(path)
    }

    fn not_utf8(&self, path: &str) -> bool {
        self.store.not_utf8(path)
    }

    fn hold(&self) -> Box<dyn Guard + '_> {
        Box::new(self.store.lock_writes())
    }

    fn baseline_names(&self) -> Vec<String> {
        self.store.read_baselines().iter().filter_map(|b| b["name"].as_str().map(str::to_string)).collect()
    }

    fn state_named(&self, current: &[FileIn], spec: &str) -> Result<(Vec<FileIn>, Value), ToolErr> {
        writes::state_named(self, current, &self.store.read_history(), spec)
    }

    fn baseline_model(&self, a: &Value) -> Result<Option<(Model, Value)>, ToolErr> {
        writes::baseline_model(self, a)
    }

    fn reviews(&self) -> Reviews {
        suspect::reviews(self)
    }

    fn rule_ignores(&self) -> Vec<Value> {
        self.store.read_rule_ignores()
    }

    fn past_states(&self, files: &[FileIn], depth: usize) -> Vec<(String, Vec<FileIn>)> {
        writes::states_back(files, &self.store.read_history(), depth)
    }

    fn suspect_pair(&self, a: &Value) -> Result<SuspectPair, ToolErr> {
        let since = a.get("since").filter(|v| !v.is_null()).cloned().unwrap_or(Value::Null);
        let args = json!({ "before": since, "after": a.get("until").cloned().unwrap_or(Value::Null) });
        let (before, after) = pair(self, &args)?;
        Ok(SuspectPair { bar_before: bar_line(&before.bar), bar_after: bar_line(&after.bar), versions: versions(&before, &after), before: before.files, after: after.files })
    }
}
