//! ComfyVault engine.
//!
//! One person runs several ComfyUI installs on one computer. The same model
//! weights sit in more than one install, so hundreds of gigabytes go to
//! duplicates. This crate scans every registered install, keeps one copy of each
//! unique file in a vault folder, and leaves a symbolic link wherever a file
//! used to be. ComfyUI keeps working, because it follows symbolic links when it
//! lists models.
//!
//! # Shape
//!
//! This crate holds all the logic and knows nothing about Tauri. The command
//! layer in `src-tauri` is a thin wrapper over it, so the same engine can back a
//! command line tool or an MCP server later.
//!
//! Every public operation is synchronous. Long work takes a
//! [`progress::ProgressSink`] and a [`progress::CancelToken`], so the caller
//! decides how to report and how to stop.
//!
//! # The rules this engine holds
//!
//! * It never moves a file it has not just checked.
//! * It never deletes bytes before the replacement is in place and verified.
//! * It writes a journal entry before each step, so a crash is recoverable.
//! * It never writes outside a registered install's model folders or the vault.
//! * It counts, and never moves, weights inside `custom_nodes` and inside the
//!   Hugging Face cache.

#![forbid(unsafe_op_in_unsafe_fn)]
#![warn(clippy::all)]

pub mod error;
pub mod install;
pub mod paths;
pub mod platform;
pub mod progress;
pub mod time_util;

pub use error::{ErrorCode, Result, VaultError};
