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

The installer lands in `target/release/bundle/nsis/`.

That path is the workspace's own `target/`, at the top of the repository.
`src-tauri` is a workspace member, so cargo writes its output there and not into
`src-tauri/target/`.

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

To confirm the interface is inside it, look for every built file name in the
bytes of the executable:

```
python3 - <<'EOF'
import os
exe = open('target/x86_64-pc-windows-msvc/release/comfyvault.exe', 'rb').read()
missing = [f for f in sorted(os.listdir('dist/assets'))
           if ('/assets/' + f).encode() not in exe]
print('missing:', missing if missing else 'none')
EOF
```

The result must be `missing: none`. Every file in `dist/assets/` is embedded,
the `.map` files included.

Do not use `strings` for this check. `strings` joins a file name to the bytes
that follow it, so an exact match fails on a name that is really there.

### 2.6 A rebuilt interface does not go into the executable by itself

Do not trust a rebuild to pick up a new interface. Tauri reads `dist/` once,
during its build script. Cargo does not rerun that build script when only
`dist/` changes. The build reports success and embeds the old interface.

This was measured. A line was added to a file in `dist/assets/`, and the
release build ran again. The build reported success in 1 minute 13 seconds.
The new line was not in the executable.

If the interface changed since the last build, force the capture:

```
touch src-tauri/build.rs src-tauri/src/lib.rs
cargo xwin build -p comfyvault --release --features custom-protocol \
  --target x86_64-pc-windows-msvc
```

Then run the check in section 2.5 again.

Do not ship a build until that check reports `missing: none`. A stale
interface fails in the same silent way as a missing `custom-protocol` feature.
The application starts, and the person uses an old screen.

Do not start a release build while the interface is being rebuilt. The build
reads `dist/` at the start and takes about 70 seconds. A write to `dist/`
during that time does not reach the executable.

This was measured. A release build started at 18:40:57. The interface was
written again at 18:41:28. The build ended at 18:42:09 and reported success.
Four of the nine built files were not in it. The five that were in it had not
changed name, so their names still matched.

Before a release build, confirm that `dist/` is at rest:

```
ls -l --time-style=+%s dist/assets
```

Run the same command after the build. If the output differs, discard the
executable and build again.

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
| `design/mock/comfyvault.html` | The design the interface was built from. Open it in a browser. |
| `docs/IPC-CONTRACT.md` | Every command, payload and event. |
| `docs/HOW-IT-WORKS.md` | What the scan reads, how the plan is decided, what Apply does. |
| `docs/FRONTEND.md` | Running the interface on its own, against a development engine. |
| `docs/TROUBLESHOOTING.md` | The stuck states, and what to do about each one. |
