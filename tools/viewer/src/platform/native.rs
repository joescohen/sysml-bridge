use std::path::PathBuf;

use crate::model::parse::parse_sysml;

/// Loads a SysML model from the local filesystem.
pub struct NativeLoader {
    path: PathBuf,
}

impl NativeLoader {
    /// Create a loader from CLI arguments, falling back to the default fixture.
    pub fn from_args() -> Self {
        let path = std::env::args()
            .nth(1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("tests/fixtures/simple-vehicle.sysml"));
        Self { path }
    }

    /// The display path for window titles.
    pub fn path_display(&self) -> String {
        self.path.display().to_string()
    }
}

impl super::ModelLoader for NativeLoader {
    fn load_model(&self) -> Result<(String, crate::model::Model), String> {
        let content = std::fs::read_to_string(&self.path)
            .map_err(|e| format!("Failed to read {}: {e}", self.path.display()))?;
        let model = parse_sysml(&content).map_err(|e| format!("Parse error: {e}"))?;
        Ok((self.path.display().to_string(), model))
    }
}
