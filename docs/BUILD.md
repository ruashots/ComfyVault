# Building ComfyVault

ComfyVault is a Windows desktop application. The engine is a plain Rust crate,
the interface is Solid built by Vite, and Tauri puts the two in one window.

---

## 1. The normal build, on Windows

Install Node 22 or newer, Rust, and the Tauri prerequisites. Then run:

```
npm install
npm run tauri build
```

The installer lands in `src-tauri/target/release/bundle/nsis/`.

`npm run tauri build` runs the interface build first, then the Rust build with
the right feature turned on. Prefer it over a hand-rolled `cargo build`.

---

## 2. Cross-building from Linux

The engine is developed on Linux. A complete Windows executable is built from
there with `cargo-xwin`.

### 2.1 What has to be installed

```
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
sudo apt install llvm clang lld
```

**LLVM is required.** Two tools in it are used:

- `llvm-rc` compiles the Windows resource that carries the icon and the version
  information. Without it the build stops with `NotAttempted("llvm-rc")`.
- `clang-cl` compiles any C code in the dependency tree.

Rust's own `llvm-tools` component does **not** include `llvm-rc`.

### 2.2 Build the interface first

```
npm install
npm run build
```

This writes `dist/`. The Rust build reads it and embeds it. If `dist/` is
missing, the Rust build still succeeds and produces an application with an
empty window, so build the interface first, every time.

### 2.3 Build the application

```
cargo xwin build -p comfyvault --release --features custom-protocol \
    --target x86_64-pc-windows-msvc
```

The executable lands at:

```
target/x86_64-pc-windows-msvc/release/comfyvault.exe
```

### 2.4 `--features custom-protocol` is not optional

Tauri embeds the interface only when that feature is on. Without it the
application looks for the development server at `http://localhost:1420`, and a
person who installs it sees an empty window.

The Tauri command line tool turns the feature on by itself. A direct
`cargo build` does not, so pass it.

### 2.5 Check the result

The executable must be a graphical program, and it must contain the interface:

```
file target/x86_64-pc-windows-msvc/release/comfyvault.exe
```

That must report `PE32+ executable (GUI) x86-64`. A report of `(console)` means
the debug profile was built.

To confirm the interface is inside it, search the executable for the built file
names:

```
strings target/x86_64-pc-windows-msvc/release/comfyvault.exe | grep '^/assets/'
```

Every file in `dist/assets/` must appear, apart from the `.map` files.

---

## 3. Building and testing the engine alone

The engine has no Tauri dependency, so it builds and tests on Linux with
nothing extra installed:

```
cargo test -p comfyvault-core
```

To type check the engine against Windows, including the Windows-only module:

```
cargo xwin build -p comfyvault-core --target x86_64-pc-windows-msvc --tests
```

That compiles the Windows test binaries as well. A mistake in them stops the
build.

### 3.1 Run the tests on Windows

**A green run on Linux is not enough.** Three real bugs passed every Linux test
and failed fifteen tests on Windows: paths that carried the `\\?\` prefix, a
scan that read the machine's real Hugging Face cache, and a database key that
told `/` and `\` apart. None of them can appear on Linux.

Build the test binary, copy it to a Windows folder, and run it. WSL runs a
Windows executable directly, so this works from Linux:

```
cargo xwin test -p comfyvault-core --no-run --target x86_64-pc-windows-msvc
cp target/x86_64-pc-windows-msvc/debug/deps/comfyvault_core-<hash>.exe \
   /mnt/c/ComfyVault-Demo/core-tests.exe
cd /mnt/c/ComfyVault-Demo && ./core-tests.exe
```

`cargo xwin test --no-run` prints the file name with the hash in it.

To run one test and see its output:

```
./core-tests.exe <test name> --exact --nocapture
```

Two things change the result, so check them before you read a failure:

- Developer Mode. With it off, every test that makes a link fails with the
  engine's `symlinkUnsupported` message. That is correct behavior, not a
  broken suite.
- The folder you run in. Use a scratch folder. Never run against a real
  ComfyUI install.

---

## 4. What a Linux build cannot do

`cargo build -p comfyvault` for Linux fails. Tauri needs GTK and D-Bus
development libraries that this project has no reason to install, because the
product targets Windows. Use the cross-build in section 2.

Three behaviors can only be tested on Windows. The engine reports each of them
honestly rather than guessing:

- Whether a symbolic link can be created. The engine measures this by creating
  one, reading it back, and deleting it.
- Whether another program holds a file open. On Linux the engine reports
  `checkable: false`, because a file there moves while it is open.
- Whether paths longer than 260 characters work.

---

## 5. Where the pieces live

| Path | What it is |
|---|---|
| `crates/comfyvault-core/` | The engine. All the rules. No Tauri. |
| `src-tauri/` | The command layer, the window, and the build settings. |
| `src/` | The interface. |
| `docs/IPC-CONTRACT.md` | Every command, payload and event. |
