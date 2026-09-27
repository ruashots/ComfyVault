//! What the window may open outside the app, driven through the real opener
//! plugin and the real capability file on the mock runtime.
//!
//! The opener refuses every address its scope does not name. The scope names
//! the Developer Mode settings page and the front page of a port on this
//! computer, and nothing else. It matches the raw text against a pattern whose
//! wildcard also matches "/", "@", "?", "#", spaces and quotes, so it cannot
//! say "a model page and nothing else". A Civitai page therefore does not go
//! through it at all: the window sends two numbers to `open_civitai_page`, and
//! the engine builds the address.
//!
//! Windows only: the window's page is served from `http://tauri.localhost`
//! there, and every call below is made from that origin. Elsewhere the page
//! has another origin, and the plugin refuses it before the scope is read.

#![cfg(windows)]

use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, INVOKE_KEY};
use tauri::webview::InvokeRequest;

fn open(url: &str) -> Result<(), String> {
    call("plugin:opener|open_url", serde_json::json!({ "url": url }))
}

/// Sends one command from the window, as the interface does.
fn call(cmd: &str, body: serde_json::Value) -> Result<(), String> {
    let app = mock_builder()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            comfyvault_lib::commands::open_civitai_page,
            comfyvault_lib::commands::open_huggingface_page
        ])
        .build(tauri::generate_context!())
        .expect("build the app");
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("the main window");
    get_ipc_response(
        &window,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            // Where the app's own page is served from on Windows. Any other
            // origin is refused before the scope is even read.
            url: "http://tauri.localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
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
        // No Civitai address goes through the window's opener, not even a real
        // model page: the window names a page by its numbers instead.
        "https://civitai.com/models/4384?modelVersionId=128713",
        "https://civitai.com/models/4384",
        // What the old rule, https://civitai.com/models/*, let through: any
        // page on the site, and spaces, quotes and new lines that reach the
        // Windows shell.
        "https://civitai.com/models/../../user/someone",
        "https://civitai.com/models/%2e%2e/%2e%2e/api/v1/models",
        "https://civitai.com/models/1?returnUrl=https://evil.example/",
        "https://civitai.com/models/1 --some-browser-flag",
        "https://civitai.com/models/1\" --some-browser-flag \"",
        "https://civitai.com/models/1\nhttps://evil.example/",
        "https://civitai.com/models/@evil.example/",
        "https://civitai.com.evil.example/models/1",
        "http://civitai.com/models/4384",
        "file:///C:/Windows/System32/calc.exe",
        "ms-settings:privacy",
        // A ComfyUI on this computer, and nowhere else.
        "http://127.0.0.1.evil.example:8188/",
        "http://127.0.0.1@evil.example/",
        "http://localhost.evil.example:8188/",
        "http://192.168.1.20:8188/",
        "http://10.0.0.1:8188/",
        "file://127.0.0.1/C$/Windows/System32/calc.exe",
        // Port 9, which nothing serves: a case the scope wrongly let through
        // would reach a real ComfyUI on its usual port.
        "https://127.0.0.1:9/",
        // Only the page itself, never a request to ComfyUI's API.
        "http://127.0.0.1:9/api/prompt",
        "http://127.0.0.1:9/?x=1",
        "http://127.0.0.1:9/#x",
        // The scope matches the raw text, and a wildcard there also matches
        // "/", "@", "?" and "#". Each of these ends in "/", as an allowed page
        // does, so only the digits-only port rule refuses them.
        // A browser reads the part before "@" as a user name, so the host is
        // evil.example, or a machine on the network.
        "http://127.0.0.1:x@evil.example/",
        "http://127.0.0.1:@evil.example/",
        "http://127.0.0.1:1@192.0.2.1/cgi-bin/reboot/",
        "http://127.0.0.1:9/api/prompt/",
        "http://127.0.0.1:9/?x=1/",
        "http://127.0.0.1:9/#/",
        "http://127.0.0.1:/",
        "http://127.0.0.1:123456/",
    ] {
        let err = open(url).expect_err(url);
        // Refused by the scope itself, not for some other reason.
        assert!(err.contains("Not allowed to open url"), "{url}: {err}");
    }
}

#[test]
fn a_civitai_page_is_named_by_numbers_and_nothing_else() {
    // Each of these fails before anything opens: the command takes whole
    // numbers, so an address, a path or extra text cannot even arrive.
    for bad in [
        serde_json::json!({ "args": { "modelId": "4384/../../user/someone" } }),
        serde_json::json!({ "args": { "modelId": "4384 --flag" } }),
        serde_json::json!({ "args": { "modelId": -1 } }),
        serde_json::json!({ "args": { "modelId": 1.5 } }),
        serde_json::json!({ "args": { "modelId": 4384, "versionId": "1?x=y" } }),
        serde_json::json!({ "args": { "url": "https://civitai.com/models/4384" } }),
    ] {
        let err = call("open_civitai_page", bad.clone()).expect_err(&bad.to_string());
        assert!(err.contains("invalid args"), "{bad}: {err}");
    }
}

#[test]
fn a_hugging_face_page_is_named_by_plain_names_and_nothing_else() {
    // Each of these is refused before anything opens: the engine builds the
    // address only from two plain Hugging Face names.
    for (owner, repo) in [
        ("..", "x"),
        ("o", "../../settings/tokens"),
        ("o/evil", "r"),
        ("o", "r?x=1"),
        ("o", "r#x"),
        ("o", "r --flag"),
        ("o\"", "r"),
        ("o", "r\nhttps://evil.example/"),
        ("evil.example@o", "r"),
        ("", "r"),
    ] {
        let err = call("open_huggingface_page", serde_json::json!({ "args": { "owner": owner, "repo": repo } }))
            .expect_err(&format!("{owner}/{repo}"));
        assert!(err.contains("not a Hugging Face model"), "{owner}/{repo}: {err}");
    }
    // The window's own opener does not open Hugging Face either.
    let err = open("https://huggingface.co/o/r").expect_err("opened");
    assert!(err.contains("Not allowed to open url"), "{err}");
}

/// Opens a real Hugging Face model page in the default browser, so it runs
/// only when asked: `--ignored`.
#[test]
#[ignore]
fn a_hugging_face_model_page_opens() {
    call(
        "open_huggingface_page",
        serde_json::json!({ "args": { "owner": "Comfy-Org", "repo": "flux1-dev" } }),
    )
    .expect("the model page did not open");
}

/// Opens a real Civitai model page in the default browser, so it runs only
/// when asked: `--ignored`.
#[test]
#[ignore]
fn a_civitai_model_page_opens() {
    call(
        "open_civitai_page",
        serde_json::json!({ "args": { "modelId": 4384, "versionId": 128713 } }),
    )
    .expect("the model page did not open");
}

/// Opens Windows' developer settings, the page where Developer Mode is turned
/// on, so it runs only when asked: `--ignored`.
#[test]
#[ignore]
fn the_developer_mode_settings_page_opens() {
    open("ms-settings:developers").expect("the Developer Mode page was refused");
}

/// Opens a page on this computer in the default browser, the way the running
/// warning does, so it runs only when asked: `--ignored`. The test serves the
/// page itself, on a port the system picks, so no real ComfyUI is ever asked.
#[test]
#[ignore]
fn a_comfyui_on_this_computer_opens() {
    use std::io::{Read, Write};
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let served = std::thread::spawn(move || {
        let (mut conn, _) = server.accept().unwrap();
        let mut buf = [0u8; 1024];
        let n = conn.read(&mut buf).unwrap();
        let body = "opener scope test, close this tab";
        let _ = write!(
            conn,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string()
    });
    open(&format!("http://127.0.0.1:{port}/")).expect("the local address was refused");
    // The browser really asked for the page, so the opener did open it.
    let request = served.join().unwrap();
    assert!(request.starts_with("GET / "), "the browser asked for {request}");
}
