//! What the window may open outside the app, driven through the real opener
//! plugin and the real capability file on the mock runtime.
//!
//! The opener refuses every address its scope does not name, and the scope
//! was empty, so "Open on Civitai" and the Developer Mode link did nothing.
//! The scope names exactly the model pages the engine builds and that one
//! settings page. Anything else must stay refused, because a page address can
//! come from a vault's cached answers.

use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, INVOKE_KEY};
use tauri::webview::InvokeRequest;

fn open(url: &str) -> Result<(), String> {
    let app = mock_builder()
        .plugin(tauri_plugin_opener::init())
        .build(tauri::generate_context!())
        .expect("build the app");
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("the main window");
    get_ipc_response(
        &window,
        InvokeRequest {
            cmd: "plugin:opener|open_url".into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            // Where the app's own page is served from on Windows. Any other
            // origin is refused before the scope is even read.
            url: "http://tauri.localhost".parse().unwrap(),
            body: InvokeBody::Json(serde_json::json!({ "url": url })),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[test]
fn addresses_outside_the_scope_are_refused() {
    for url in [
        "https://evil.example/",
        "https://civitai.com/user/someone",
        "https://civitai.com.evil.example/models/1",
        "http://civitai.com/models/4384",
        "file:///C:/Windows/System32/calc.exe",
        "ms-settings:privacy",
    ] {
        let err = open(url).expect_err(url);
        // Refused by the scope itself, not for some other reason.
        assert!(err.contains("Not allowed to open url"), "{url}: {err}");
    }
}

/// Opens a real Civitai page in the default browser, so it runs only when
/// asked: `--ignored`.
#[test]
#[ignore]
fn a_civitai_model_page_opens() {
    open("https://civitai.com/models/4384?modelVersionId=128713").expect("the model page was refused");
}
