fn main() {
    // The application gets its manifest from Tauri's own resource, which is
    // linked into the program only. A test program of this crate links the
    // same window code and, without a manifest naming Common Controls 6,
    // Windows refuses to start it ("Entry Point Not Found"). This one is
    // linked into the test programs alone, so the shipped program is as before.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-test-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}", manifest.display());
    }
    tauri_build::build()
}
