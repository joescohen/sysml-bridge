use sysmlv2_gui::platform::ModelLoader;

// --- Native entry point ---------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    env_logger::init();

    let loader = sysmlv2_gui::platform::native::NativeLoader::from_args();
    let (name, model) = loader.load_model().unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    log::info!(
        "Parsed {} elements, {} relationships",
        model.element_count(),
        model.relationships.len()
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("SysML v2 Viewer - {name}"))
            .with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };

    eframe::run_native(
        "sysmlv2-gui",
        options,
        Box::new(|_cc| Ok(Box::new(sysmlv2_gui::app::SysmlApp::new(model)))),
    )
}

// --- WASM entry point -----------------------------------------------------

#[cfg(target_arch = "wasm32")]
fn main() {
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let loader = sysmlv2_gui::platform::web::WebLoader;
    let (_name, model) = loader.load_model().expect("embedded demo model must parse");

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async move {
        eframe::WebRunner::new()
            .start(
                "the_canvas_id",
                web_options,
                Box::new(move |_cc| Ok(Box::new(sysmlv2_gui::app::SysmlApp::new(model)))),
            )
            .await
            .expect("failed to start eframe");
    });
}
