# Building ComfyVault

ComfyVault is a Windows desktop program. The engine is a plain Rust crate, the
interface is Solid built by Vite, and Tauri puts the two in one window.

The build runs on Linux, or in WSL on Windows, and cross-compiles the Windows
program with `cargo-xwin`. The result is one portable file, `comfyvault.exe`.
There is no installer.

---

## 1. What has to be installed

- Node 22.13 or newer. The repository has an `.nvmrc`.
- Rust, from [rustup.rs](https://rustup.rs).
- `cargo-xwin`, the Windows target, and LLVM:

```
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
sudo apt install llvm clang lld
```

**LLVM is required.** The build uses two tools from it:

- `llvm-rc` compiles the Windows resource that carries the icon and the version
  information. Without it, the build stops with `NotAttempted("llvm-rc")`.
- `clang-cl` compiles the C code in the dependency tree.

Rust's own `llvm-tools` component does **not** include `llvm-rc`.

### If you cannot install packages

`apt-get download` needs no root. Unpack the packages into a folder of your
own and put that folder's `bin` on `PATH`:

```
mkdir -p ~/llvm-portable && cd ~/llvm-portable
apt-get download llvm-18 clang-18 libclang-cpp18 libllvm18 lld-18 \
  libclang-common-18-dev libgcc-13-dev
for d in *.deb; do dpkg-deb -x "$d" root; done
ln -sf clang root/usr/lib/llvm-18/bin/clang-cl
export PATH="$PWD/root/usr/lib/llvm-18/bin:$PATH"
```

The download is about 71 MB, and the unpacked folder is about 329 MB. The
package does not include a `clang-cl`, and `cargo-xwin` looks for that name, so
the `ln` line makes one.

Check both tools before you build:

```
llvm-rc --version
clang-cl --version
```

`llvm-rc` answers `Exactly one input file should be provided`, which means it
runs. `clang-cl` prints a version. If either says `command not found`, `PATH`
is not set in this shell. `export PATH` lasts only for the shell that ran it.

---

## 2. Build the program

### 2.1 Build the interface first

```
npm install
npm run build
```

This type-checks the interface and writes `dist/`. The Rust build reads `dist/`
and embeds it. If `dist/` is missing, the Rust build still succeeds, and the
program shows an empty window. Build the interface first, every time.

### 2.2 Build the executable

```
cargo xwin build -p comfyvault --release --features custom-protocol \
    --target x86_64-pc-windows-msvc
```

The program is at:

```
target/x86_64-pc-windows-msvc/release/comfyvault.exe
```

That one file is the whole program. Copy it to any folder on the Windows PC and
run it. It keeps one small file outside the vault, which vault folder to open,
in `%APPDATA%\app.comfyvault.desktop\config.json`.

### 2.3 `--features custom-protocol` is not optional

Tauri embeds the interface only when that feature is on. Without it, the
program looks for the development server at `http://localhost:1420`, and shows
an empty window.

### 2.4 Check the result

The executable must be a graphical program:

```
file target/x86_64-pc-windows-msvc/release/comfyvault.exe
```

The answer must be `PE32+ executable (GUI) x86-64`. An answer of `(console)`
means that the debug profile was built.

The executable must also contain the interface. Look for every built file name
in its bytes:

```
python3 - <<'EOF'
import os
exe = open('target/x86_64-pc-windows-msvc/release/comfyvault.exe', 'rb').read()
missing = [f for f in sorted(os.listdir('dist/assets'))
           if ('/assets/' + f).encode() not in exe]
print('missing:', missing if missing else 'none')
EOF
```

The result must be `missing: none`.

The check looks for names, not contents, because the contents are compressed
inside the executable. Names are enough: the interface build makes each file
name from a hash of that file's contents, so new contents get a new name.

Do not use `strings` for this check. `strings` joins a file name to the bytes
that follow it, so an exact match fails on a name that is really there.

### 2.5 A rebuilt interface does not reach the executable by itself

Tauri reads `dist/` once, in its build script. Cargo does not run that build
script again when only `dist/` changes. The build reports success and embeds the
old interface.

If the interface changed since the last build, make Cargo run the build script
again:

```
touch src-tauri/build.rs src-tauri/src/lib.rs
cargo xwin build -p comfyvault --release --features custom-protocol \
  --target x86_64-pc-windows-msvc
```

Then run the check in section 2.4 again. Do not ship a build until it reports
`missing: none`. A stale interface fails silently: the program starts, and
shows an old screen.

Do not change `dist/` while a release build runs. The build reads `dist/` at
the start, and a later write does not reach the executable. To make sure that
`dist/` did not change, run this before and after the build:

```
ls -l --time-style=+%s dist/assets
```

If the two outputs differ, discard the executable and build again.

---

## 3. The tests

### 3.1 The engine

The engine has no Tauri dependency, so it builds and tests on Linux with
nothing extra installed:

```
cargo test -p comfyvault-core
```

To type-check the engine against Windows, including its Windows-only code and
test binaries:

```
cargo xwin build -p comfyvault-core --target x86_64-pc-windows-msvc --tests
```

### 3.2 The interface

```
npm test
```

### 3.3 Run the engine tests on Windows

**A green run on Linux is not enough.** Paths with the `\\?\` prefix, the
Hugging Face cache, and the difference between `/` and `\` in a stored path
only go wrong on Windows.

Build the test program, copy it to a scratch folder on the Windows drive, and
run it. WSL runs a Windows program directly, so this works from WSL:

```
cargo xwin test -p comfyvault-core --no-run --target x86_64-pc-windows-msvc
cp "$(ls -t target/x86_64-pc-windows-msvc/debug/deps/comfyvault_core-*.exe | head -1)" \
   /mnt/c/<scratch folder>/core-tests.exe
cd /mnt/c/<scratch folder>
COMFYVAULT_REPO='<the repository folder, as Windows sees it>' \
  WSLENV=COMFYVAULT_REPO ./core-tests.exe
```

For a repository in WSL, the Windows path looks like
`\\wsl.localhost\<distribution>\home\<you>\ComfyVault`.

Always take the newest test program, never a name you remember. The hash in
the name changes when the crate's settings change, and the old file stays
beside the new one. An old file runs old tests, and they pass.

Three tests read files from the repository: the samples in `docs/golden/`, the
contract, and the command layer's source. `COMFYVAULT_REPO` tells the test
program where the repository is. `WSLENV` is not optional: without it, WSL does
not pass the variable to a Windows program, and those three tests fail.

To run one test and see its output:

```
./core-tests.exe <test name> --exact --nocapture
```

Before you read a failure, check two things:

- **Developer Mode.** If it is off, every test that makes a link fails with the
  engine's `symlinkUnsupported` message. That is correct behavior.
- **The folder you run in.** Use a scratch folder. Never run the tests against
  a real ComfyUI install.

### 3.2 What the window may open

`src-tauri/tests/opener_scope.rs` drives the real opener plugin through the
real capability file, on Tauri's mock runtime. Build it and run it on Windows:

```
cargo xwin test -p comfyvault --test opener_scope --no-run --target x86_64-pc-windows-msvc
cp "$(ls -t target/x86_64-pc-windows-msvc/debug/deps/opener_scope-*.exe | head -1)" \
   /mnt/c/<scratch folder>/opener-tests.exe
cd /mnt/c/<scratch folder>
./opener-tests.exe
./opener-tests.exe --ignored
```

The first run checks that addresses outside the scope are refused. The second
opens a real Civitai model page in the default browser, so it runs only when
asked.

The test program gets its own Windows manifest from `src-tauri/build.rs`. The
shipped program does not use it. Without that manifest Windows refuses to start
the test program, with an "Entry Point Not Found" message.

---

## 4. What a Linux build cannot do

`cargo build -p comfyvault` for Linux fails. Tauri needs GTK and D-Bus
development libraries that this project does not install, because the product
is for Windows. Use the cross build in section 2.

Three behaviors can only be tested on Windows. The engine reports each of them
as a measurement, not a guess:

- Whether a symbolic link can be made. The engine makes one, reads it back, and
  deletes it.
- Whether another program holds a file open. On Linux the engine reports
  `checkable: false`, because a file there can move while it is open.
- Whether paths longer than 260 characters work.

One behavior has not been tested on real hardware: a vault on a different
drive from an install. The engine tests simulate the drive boundary. No test
has moved files to a real second drive and back.

---

## 5. Where the pieces are

| Path | What it is |
|---|---|
| `crates/comfyvault-core/` | The engine. All the rules. No Tauri. |
| `src-tauri/` | The command layer, the window, and the build settings. |
| `src/` | The interface. |
| `design/mock/comfyvault.html` | The design mock the interface was built from. Open it in a browser. |
| `docs/IPC-CONTRACT.md` | Every command, payload and event. |
| `docs/HOW-IT-WORKS.md` | What the scan reads, how the plan is made, what Apply and Undo do. |
| `docs/FRONTEND.md` | Running the interface on its own, against a development engine. |
| `docs/TROUBLESHOOTING.md` | The stuck states, and what to do about each one. |
