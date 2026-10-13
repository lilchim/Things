//! Things: a personal desktop journal, games first.

mod app;
mod components;

use std::path::PathBuf;

use things_core::Store;

fn main() -> eframe::Result {
    let root = data_dir();
    let store = match Store::open(&root) {
        Ok(store) => store,
        Err(e) => {
            eprintln!("Can't open data folder: {e}");
            std::process::exit(1);
        }
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Things")
            .with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Things",
        options,
        Box::new(|_cc| Ok(Box::new(app::ThingsApp::new(store)))),
    )
}

/// The data folder: the first argument, else `$THINGS_DIR`, else `~/things`.
fn data_dir() -> PathBuf {
    if let Some(arg) = std::env::args_os().nth(1) {
        return arg.into();
    }
    if let Some(dir) = std::env::var_os("THINGS_DIR") {
        return dir.into();
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    home.map(PathBuf::from).unwrap_or_default().join("things")
}
