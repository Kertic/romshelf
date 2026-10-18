//! The `romshelf` binary: argument parsing and the Tauri shell over `romshelf-core`.

pub mod cli;
mod gamepad;
mod state;

use clap::Parser;
use cli::Cli;
use romshelf_core::library::{self, LibraryEntry};
use serde::Serialize;
use state::AppState;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Parses arguments and runs; returns the process exit code.
pub fn run() -> i32 {
    let cli = Cli::parse();
    match cli.command {
        None => run_interface(),
        Some(cmd) => {
            eprintln!(
                "romshelf {}: not implemented yet in this build.",
                cmd.name()
            );
            2
        }
    }
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    fake: bool,
    data_dir: String,
}

#[tauri::command]
fn app_info(state: tauri::State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        fake: state.fake,
        data_dir: state.paths.data_dir.display().to_string(),
    }
}

#[tauri::command]
fn library(state: tauri::State<'_, AppState>) -> Result<Vec<LibraryEntry>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    library::entries(&db, &state.registry).map_err(|e| e.to_string())
}

#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| v == "1")
}

fn run_interface() -> i32 {
    #[cfg(target_os = "linux")]
    {
        // WebKitGTK's DMA-BUF renderer shows a blank window under gamescope on some drivers.
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }

    let state = match AppState::new(env_flag("ROMSHELF_FAKE")) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Couldn't open Romshelf's data: {e}");
            return 1;
        }
    };
    // Full screen unless asked otherwise; debug builds default to a window for development.
    let fullscreen = if env_flag("ROMSHELF_WINDOWED") {
        false
    } else {
        !cfg!(debug_assertions) || env_flag("ROMSHELF_FULLSCREEN")
    };

    let result = tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![app_info, library, quit])
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Romshelf")
                .inner_size(1280.0, 800.0)
                .fullscreen(fullscreen)
                .focused(true)
                .build()?;
            gamepad::start(app.handle().clone());
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Romshelf couldn't start its window: {e}");
            1
        }
    }
}
