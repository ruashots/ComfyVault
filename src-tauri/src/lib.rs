//! The ComfyVault desktop application.
//!
//! This crate is deliberately thin. It holds no rules about models, vaults or
//! links: all of that lives in `comfyvault-core`, which knows nothing about
//! Tauri. This layer does three things and nothing else:
//!
//! 1. Turns each command in `docs/IPC-CONTRACT.md` into a call on the engine.
//! 2. Runs disk work on a blocking thread, so the window never freezes.
//! 3. Turns the engine's progress into the events the interface listens for.
//!
//! Keeping it this thin is what lets the same engine back a command line tool
//! or an MCP server later without moving any logic.

pub mod commands;
mod events;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use comfyvault_core::engine::Engine;
use tauri::Manager;

/// The engine, shared by every command.
pub struct AppEngine(pub Arc<Engine>);

/// Where the application remembers which vault to open.
///
/// Only the vault folder lives here. Everything else lives inside the vault, so
/// the vault folder is self describing and moving the drive moves the record
/// with it.
fn config_path(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config.json")
}

/// A second launch arrived before the window was ready to be shown.
///
/// Two quick clicks on the program can hand over while the first copy is still
/// in `setup`, before its window is registered. The request is kept here and
/// served when `setup` ends, rather than dropped.
static FOCUS_PENDING: AtomicBool = AtomicBool::new(false);

/// Shows the open window in front of everything, restored if it was
/// minimized. The second copy grants the right to take the foreground before
/// it exits, which is what lets this work from a program in the background.
fn bring_to_front(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        FOCUS_PENDING.store(true, Ordering::SeqCst);
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn run() {
    // One copy of the program at a time. A second launch hands over to the
    // window that is already open and exits. Tauri starts plugins before it
    // makes any window or runs `setup`, so the second copy never opens a
    // window and never touches the vault. It must stay the first plugin.
    //
    // Besides it, only the opener plugin is registered. Browsing the disk goes
    // through the engine's own list_directory and create_directory, so the
    // window holds no file system permission of its own.
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            bring_to_front(app)
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let engine = Engine::new(config_path(&app.handle().clone()));
            engine.set_download_sink(std::sync::Arc::new(events::EventSink::<comfyvault_core::download::Download>::new(
                app.handle().clone(),
                events::DOWNLOAD_PROGRESS,
            )));

            // Open last time's vault, if it is still there. A vault on a drive
            // that is not plugged in must not stop the window from appearing,
            // so a failure is reported to the interface rather than raised.
            if let Some(problem) = engine.restore_last_vault() {
                let handle = app.handle().clone();
                events::emit_startup_problem(&handle, &problem);
            }

            app.manage(AppEngine(engine));
            if FOCUS_PENDING.swap(false, Ordering::SeqCst) {
                bring_to_front(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_platform_report,
            commands::list_drives,
            commands::get_app_state,
            commands::select_vault,
            commands::close_vault,
            commands::get_settings,
            commands::update_settings,
            commands::validate_install_path,
            commands::register_install,
            commands::list_installs,
            commands::refresh_install,
            commands::update_install,
            commands::unregister_install,
            commands::list_install_model_dirs,
            commands::start_scan,
            commands::get_scan_entries,
            commands::cancel_scan,
            commands::get_last_scan,
            commands::build_plan,
            commands::get_plan,
            commands::start_apply,
            commands::cancel_apply,
            commands::get_apply_result,
            commands::list_applies,
            commands::get_interrupted_applies,
            commands::resume_apply,
            commands::set_aside_run,
            commands::preview_revert,
            commands::revert_apply,
            commands::create_link,
            commands::remove_link,
            commands::create_model_folder,
            commands::list_link_folders,
            commands::create_link_folder,
            commands::list_links,
            commands::list_vault_files,
            commands::list_contents,
            commands::get_vault_info,
            commands::list_name_groups,
            commands::set_canonical_name,
            commands::plan_unify_name,
            commands::unify_name,
            commands::undo_unify_name,
            commands::get_hidden_name_cards,
            commands::set_hidden_name_cards,
            commands::remove_alias,
            commands::list_orphans,
            commands::delete_vault_file,
            commands::check_vault_health,
            commands::remove_dangling_links,
            commands::check_model_usage,
            commands::get_metadata,
            commands::fetch_metadata_batch,
            commands::clear_metadata_cache,
            commands::get_running_comfy,
            commands::open_task_manager,
            commands::open_civitai_page,
            commands::set_token,
            commands::get_token_status,
            commands::remove_token,
            commands::read_model_address,
            commands::start_download,
            commands::stop_download,
            commands::continue_download,
            commands::discard_download,
            commands::remove_download,
            commands::list_downloads,
            commands::open_huggingface_page,
            commands::check_locked_files,
            commands::list_directory,
            commands::create_directory,
        ])
        .run(tauri::generate_context!())
        .expect("ComfyVault could not start");
}
