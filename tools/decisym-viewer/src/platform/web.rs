use crate::model::parse::parse_sysml;

const DEMO_MODEL: &str = include_str!("../../tests/fixtures/simple-vehicle.sysml");

/// Loads an embedded demo SysML model for the web build.
pub struct WebLoader;

impl super::ModelLoader for WebLoader {
    fn load_model(&self) -> Result<(String, crate::model::Model), String> {
        let model = parse_sysml(DEMO_MODEL).map_err(|e| format!("Parse error: {e}"))?;
        Ok(("Simple Vehicle Model (demo)".to_string(), model))
    }
}
