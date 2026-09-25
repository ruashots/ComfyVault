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
