# ComfyVault

**Keep one copy of every ComfyUI model on your Windows PC.** ComfyVault finds
the model files that sit in more than one of your ComfyUI installs, keeps one
copy of each in a single vault folder, and deletes the duplicate copies. A link
takes the place of every file it moved, so ComfyUI loads each model from the
same path as before.

**Before.** The same LoRA in three installs, under three different folders:

```
C:\ComfyUI-A\models\loras\awesome\style.safetensors      144 MB
C:\ComfyUI-B\models\loras\new\style.safetensors          144 MB
C:\ComfyUI-C\models\loras\style.safetensors              144 MB
```

**After.** One real file, three links:

```
C:\ComfyVault\loras\style.safetensors                    144 MB   the only copy

C:\ComfyUI-A\models\loras\awesome\style.safetensors  ->  the vault file
C:\ComfyUI-B\models\loras\new\style.safetensors      ->  the vault file
C:\ComfyUI-C\models\loras\style.safetensors          ->  the vault file
```

288 MB goes back to the drive. Every path is the path it was before, so no
workflow changes and no ComfyUI setting changes.

Every step goes into a journal before it happens. You can stop a run, finish a
run that a crash cut off, and undo a run.

---

## Who this is for

You run more than one ComfyUI install on one Windows PC. A launcher made one, a
tutorial made another, and a portable build is still in Downloads. Each install
has its own `models` folder, and the same weights sit in more than one of them.
Nothing tells you which files are duplicates, so you keep all of them.

If you run a single install and never download the same model twice, ComfyVault
has little to do for you.

---

## Before you start

**ComfyVault runs on Windows only.** There is no macOS or Linux version of the
program.

**Developer Mode must be on.** Windows does not let an ordinary program create a
symbolic link without it. Open Settings, then System, then For developers, and
turn on Developer Mode. On Windows 10 the page is under Update & Security. You
do not have to restart.

ComfyVault checks this itself: it makes a test link, reads it back, and deletes
it. If the test fails, Apply stays blocked and the app tells you why.

**Microsoft Edge WebView2 draws the window.** Windows 11 includes it, and most
Windows 10 PCs have it through Edge. If the window does not open, install the
WebView2 Runtime from Microsoft.

---

## Get it

There is no installer and no download yet. You build ComfyVault from source,
and the result is one portable program, `comfyvault.exe`. It needs no install
step. Put it where you like and run it.

The program runs on Windows, but the tested build is made on Linux, or in WSL
(the Linux that Windows can run inside itself), and it produces the Windows
program from there. In short:

```
git clone https://github.com/Ruashots/ComfyVault
cd ComfyVault
npm install
npm run build
cargo xwin build -p comfyvault --release --features custom-protocol \
    --target x86_64-pc-windows-msvc
```

The program is then at
`target/x86_64-pc-windows-msvc/release/comfyvault.exe`.

You need Node 22.13 or newer, Rust, `cargo-xwin` and LLVM. The full steps, and
the check that tells you the build is good, are in
[docs/BUILD.md](docs/BUILD.md).

---

## How a consolidation goes

1. **Choose the vault folder.** This is step one on the first run, and nothing
   else opens until it is done. The vault is one plain folder, and there is no
   default: you choose where it goes. The folder must be empty or not exist
   yet. A folder that already holds files is refused, and nothing in it
   changes. Use **New folder** in the folder picker to make an empty one. The
   vault cannot be inside a ComfyUI install, and ComfyVault refuses that choice.

   - **Put it on the same drive as your installs.** On that drive, files are
     moved by a rename, which is instant, so the vault needs no free space of
     its own. On any other drive, every file is copied across first, so that
     drive needs the room before the run starts.
   - **Do not put it on a removable drive or a network drive.** Every install
     points into the vault by link. On any day that drive is missing, every
     model in every install stops loading at once. ComfyVault warns you and
     lets you choose it anyway.
   - **A vault on a different drive from an install is not proven.** Moving
     files to another drive, and undoing that, has only been tested against a
     simulated drive boundary, never a real second drive. If you choose one,
     keep your own backup of those models until you have checked a run and an
     undo yourself.

2. **Add your installs.** ComfyVault checks that a folder is really a ComfyUI
   install before it accepts it, and it finds the real install folder inside a
   launcher layout. Add as many as you have.

3. **Close ComfyUI, then scan.** The scan reads every model file in every
   install and calculates a SHA-256 hash of each one. The first scan reads
   every byte, so a terabyte of models takes a while. Later scans reuse the
   hash of each file that did not change.

4. **Read the plan.** The Consolidate screen shows a dry run. Nothing has moved
   yet. It lists every model that exists more than once, which copy goes into
   the vault, which paths become links, and how much space comes back. Untick
   anything you want left alone.

5. **Apply.** ComfyVault moves the files, makes the links, and deletes the
   duplicate copies. You can press **Stop now** at any time. Each model is done
   completely or not at all, and each path always holds its own file or a
   working link, so every model keeps loading in ComfyUI during the run.

**Later runs.** Scan again whenever you add models or installs, and
Consolidate shows a new plan. A new copy of a model the vault already holds
moves nothing in: the copy is checked against the vault file, deleted, and
replaced by a link to that file.

---

## Undo, and what it costs

Every run can be undone. Before an undo starts, ComfyVault shows what it will
cost, drive by drive, and it refuses to start if a drive does not have the room.

- **The copy the vault kept comes back instantly**, by a rename. On a vault that
  is on another drive, this copy is copied back too.
- **Each deleted duplicate is copied back out of the vault.** The run deleted
  those copies to free the room, so the undo has to write them again. This is
  what takes the time and the disk space.
- **The copies keep what the drive knew about the file.** A sparse file stays
  sparse, an NTFS compressed file stays compressed, and each file gets back its
  original modification time.
- **A copy that a run linked to a vault file from an earlier run** is copied
  back, and the vault file stays, because the earlier run still owns it.

An undo can be stopped at any point too, and every model still loads while it is
stopped. **Undo the rest** finishes it later.

If the PC crashes or the app closes during a run, ComfyVault shows that run the
next time it opens. You can finish it or undo it. Nothing else starts until you
choose.

---

## What it will not do

**It does not download models.** The Download screen says so. Downloads from
Hugging Face and Civitai are not in this version. Download the way you always
do, then scan again.

**Consolidated models show no picture in ComfyUI's model browser**, from
ComfyUI 0.28 on. ComfyUI refuses to serve a preview image through a per-file
link. This is a security fix in ComfyUI, and it will not be reverted. Loading
the model and running a workflow are not affected. ComfyVault reads each
install's version and tells you which installs this affects.

**"Not used" is not a safe-to-delete list.** ComfyVault can search your saved
workflow files for a model's file name. It is a plain-text search inside the
JSON. It does not read the graph. A workflow that was never saved lives in the
browser, and ComfyVault cannot see it. When there are no saved workflow files,
the app says so rather than calling every model unused.

**It never moves files inside `custom_nodes` or the Hugging Face cache.** It
counts them so the totals add up, and leaves them where they are. A custom node
loads its own weights from its own folder, and the Hugging Face libraries manage
their own cache.

**It only moves model weight files.** The scan takes files with one of these
extensions, and over 1 MB in size:
`.safetensors` `.ckpt` `.pt` `.pth` `.bin` `.gguf` `.onnx` `.pt2` `.sft` `.pkl`.
Settings shows these rules. This version has no control to change them.

**It runs one long job at a time.** A scan, a run and an undo are long jobs.
ComfyVault refuses a second one while one runs, so two jobs never touch the
same files.

---

## How it keeps your models safe

**Nothing moves until you press Apply.** The plan only reads the disk.

**Each file is checked again just before it is touched.** ComfyVault compares
the size and the modification time with what the scan recorded. If either
changed, it leaves that file alone and reports it.

**Bytes are never deleted before the link is in place.** For each duplicate,
ComfyVault renames the file aside, makes the link, and deletes the renamed file
last. If a step fails, the file goes back under its own name.

**A duplicate is read again before it is deleted.** By default, ComfyVault reads
the duplicate one more time and compares it with the copy it keeps. The delete
is the only step that removes bytes, so it happens on proof, not on a hash from
an earlier scan. You can turn this off in Settings to go faster.

**A move to another drive is copy, check, then delete.** The copy is written,
flushed to the disk, read back and hashed. Only then is the original removed.

**Nothing is ever overwritten.** If something already sits where a file or a
link would go, ComfyVault stops and reports it.

The full detail is in [docs/HOW-IT-WORKS.md](docs/HOW-IT-WORKS.md).

---

## The screens

**Home** walks you through the two setup steps on the first run: the vault,
then your installs. After a scan, it shows how many installs, how many unique
models, how much is on disk, and how much can come back.

**Library** has one row per unique model, whether it is already in the vault or
still in four installs. Search it, sort it by how many places hold a file, and
see every name a model has. Open a model to see what Civitai knows about it:
its name, base model, trigger words, and a link to its Civitai page.

**Consolidate** shows the dry run and the Apply button, then the finished run
with its undo.

**Cleanup** handles what goes wrong later. Links that point at a missing file
come first, and you can remove them. A file that arrived under two names can
keep the name you pick, and the other name stays as a link so saved workflows
still open. Vault files that nothing points at come last.

**Download** is empty in this version, and says so.

**Settings** holds your installs, the vault folder, the scan rules and the
Civitai switch. It also checks Developer Mode again, and which ComfyUI programs
are running.

---

## Your data

The vault is a plain folder you choose. For example, a vault you made at
`C:\ComfyVault` looks like this:

```
C:\ComfyVault\
  checkpoints\            one folder per model category
  loras\
  vae\
  .comfyvault\
    vault.redb            the record of every install, scan and run
```

The record lives inside the vault, so it moves with the drive. The only thing
kept outside is which vault folder to open, in
`%APPDATA%\app.comfyvault.desktop\config.json`.

**ComfyVault sends each model's fingerprint to Civitai, and you can turn that
off.** After a scan, ComfyVault sends the SHA-256 hash of each model file
it has not asked about before to Civitai, to find the model's name, base model,
trigger words and page. No file name and no path is sent. The lookup
is on by default. **Turn it off with the Civitai lookup switch in Settings**,
and ComfyVault uses no network at all. Everything else works the same without
it.

There is no account, no API key and no telemetry.

---

## Documentation

| Document | What it covers |
|---|---|
| [docs/HOW-IT-WORKS.md](docs/HOW-IT-WORKS.md) | What the scan reads, how the plan is made, what Apply and Undo do step by step |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Developer Mode, a blocked Apply, broken links, missing pictures, a run cut off by a crash |
| [docs/BUILD.md](docs/BUILD.md) | Building the program, checking the build, running the tests |
| [docs/IPC-CONTRACT.md](docs/IPC-CONTRACT.md) | Every command, payload, error and event between the window and the engine |
| [docs/FRONTEND.md](docs/FRONTEND.md) | Running the interface on its own, against a development engine |

---

## How it is built

The engine is a plain Rust crate, `crates/comfyvault-core`. It holds every rule
about models, vaults and links, and it knows nothing about the window. The
interface is Solid and TypeScript. Tauri puts the two in one window. Each half
has its own test suite, and the engine's suite also runs on Windows, where links
and locked files behave differently from Linux.
