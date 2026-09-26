//! Turning the engine's progress into the events the interface listens for.
//!
//! The names and payloads are the ones in `docs/IPC-CONTRACT.md` section 13.
//! The engine itself knows nothing about events: it calls a sink, and this is
//! the sink that puts the update on the window.

use std::marker::PhantomData;

use comfyvault_core::progress::ProgressSink;
use comfyvault_core::VaultError;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const SCAN_PROGRESS: &str = "scan:progress";
pub const SCAN_DONE: &str = "scan:done";
pub const SCAN_ERROR: &str = "scan:error";
pub const APPLY_PROGRESS: &str = "apply:progress";
pub const APPLY_DONE: &str = "apply:done";
pub const APPLY_ERROR: &str = "apply:error";
pub const REVERT_PROGRESS: &str = "revert:progress";
pub const REVERT_DONE: &str = "revert:done";
pub const REVERT_ERROR: &str = "revert:error";
/// Every change to a download: its state, and its bytes up to four times a
/// second. The payload is the whole `Download`.
pub const DOWNLOAD_PROGRESS: &str = "download:progress";
/// Raised when the vault remembered from last time could not be opened.
pub const STARTUP_PROBLEM: &str = "app:startup-problem";

/// A sink that puts every update on the window.
pub struct EventSink<T> {
    app: AppHandle,
    event: &'static str,
    _payload: PhantomData<T>,
}

impl<T> EventSink<T> {
    pub fn new(app: AppHandle, event: &'static str) -> Self {
        Self { app, event, _payload: PhantomData }
    }
}

impl<T: Serialize + Clone + Send + Sync> ProgressSink<T> for EventSink<T> {
    fn emit(&self, update: &T) {
        // A window that has gone away is not a failure worth stopping work
        // for: the operation keeps running and finishes cleanly.
        let _ = self.app.emit(self.event, update);
    }
}

/// Sends the result of a long operation, on the right event for how it ended.
pub fn emit_result<T: Serialize + Clone>(
    app: &AppHandle,
    done_event: &str,
    error_event: &str,
    result: comfyvault_core::Result<T>,
) {
    match result {
        Ok(value) => {
            let _ = app.emit(done_event, value);
        }
        Err(e) => {
            let _ = app.emit(error_event, e);
        }
    }
}

pub fn emit_startup_problem(app: &AppHandle, problem: &VaultError) {
    let _ = app.emit(STARTUP_PROBLEM, problem);
}
