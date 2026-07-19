/// Platform abstraction layer.
///
/// Native builds load models from the filesystem; web builds
/// embed a demo model at compile time.
#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod web;

/// Load a SysML model from a platform-specific source.
pub trait ModelLoader {
    /// Returns (display_name, parsed_model).
    fn load_model(&self) -> Result<(String, crate::model::Model), String>;
}
