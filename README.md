# ComfyVault

**Keep one copy of every ComfyUI model on your PC.** ComfyVault moves each model
weight into a single vault folder and leaves a link in every place the file used
to be. ComfyUI keeps loading the same models from the same paths, and the
duplicate copies stop taking up room.

![ComfyVault Home, after a scan of three ComfyUI installs](docs/img/home.png)

---

## The problem this solves

You run more than one ComfyUI install. A launcher put one there. A tutorial made
you clone another. The portable build is still in Downloads.

Every one of them has its own `models` folder, and the same weights sit in more
than one of them. The same 6 GB checkpoint, twice. The same LoRA, three times.
Nothing tells you which files are duplicates, so you keep them all, and the drive
fills up.

One measured example: a 1.82 TB drive, 93 percent full, holding about 600 GB of
duplicate copies.

ComfyVault reads every install you register, works out which files are byte for
byte identical, keeps one copy, and links the rest.

---

## What happens to your files

**Before.** The same LoRA, in three installs, under three different folders:

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

288 MB goes back to the drive. Every path is the path it always was, so no
workflow changes, no ComfyUI setting changes, and every install still lists the
model exactly where it listed it before.

The copy that moves gets a link in its old place too. The rule is simple:
wherever a file was, a link takes its place.

---

## Before you start

**Windows only.** The engine is cross platform, but the product is built,
tested, and shipped for Windows. There is no macOS or Linux build.

**Windows Developer Mode must be on.** Windows refuses to let an ordinary
program create a symbolic link without it. Open Settings, go to System, then For
developers, and turn Developer Mode on. No restart is needed.

ComfyVault checks this by creating a real link, reading it back, and deleting
it. It does not guess from your Windows version. If the check fails, Apply is
blocked and the app tells you why.

---

## Install

Build it from source. There is no prebuilt installer yet.

```
git clone https://github.com/Ruashots/ComfyVault
cd ComfyVault
npm install
npm run tauri build
```

The installer lands in `target\release\bundle\nsis\`.

You need [Node 22 or newer](https://nodejs.org), [Rust](https://rustup.rs), and
the Tauri prerequisites for Windows. Full build notes, including the cross build
from Linux, are in [docs/BUILD.md](docs/BUILD.md).

---

## The first five minutes

1. **Choose the vault folder first.** Open Settings and pick it. The vault is
   one plain folder, and it can sit on any drive. It cannot sit inside a ComfyUI
   install, and ComfyVault refuses that choice. Nothing else works until a vault
   is open, because the vault is where ComfyVault keeps its record.

2. **Add your installs.** Browse to a ComfyUI folder. The picker tells you
   whether the folder is a real ComfyUI install before you commit to it, and it
   finds the root inside a launcher layout for you. Add as many as you have.

3. **Close ComfyUI, then press Scan.** ComfyVault reads every model file in
   every install and takes a SHA-256 hash of each one. A first scan over a
   terabyte takes a while, because every byte is read. Later scans reuse the
   hashes of files that have not changed.

4. **Read the plan.** The Consolidate screen shows a dry run. Nothing has moved.
   It lists every model that exists more than once, which copy would move into
   the vault, which paths would become links, and how much space comes back.
   Untick anything you want left alone.

5. **Press Apply.** ComfyVault moves the files, creates the links, and deletes
   the duplicate copies. Every step is written to a journal first, so a crash is
   recoverable. When it is done you can undo the whole run.

![The Consolidate screen: a dry run, before anything moves](docs/img/consolidate.png)

![The same run, finished: what moved, what was linked, and the button that undoes it](docs/img/apply-done.png)

---

## What it will not do

This is the part worth reading before you install anything.

**It does not download models.** The Download screen exists and says so. Hugging
Face and Civitai downloads are not in this version. Download the way you always
have, then run a scan.

**Consolidated models lose their thumbnail in ComfyUI's model browser.** From
ComfyUI 0.28.0 onward, the route that serves preview images rejects a file
reached through a per-file symbolic link. This is a security fix in ComfyUI. It
will not be reverted, and it is the price of per-file links.

Loading the model is not affected. Running a workflow is not affected. Only the
little preview picture in the model browser goes away. ComfyVault reads each
install's version and tells you which of your installs this affects.

**"Not used" is not a safe-to-delete list.** ComfyVault can search your saved
workflow files for a model's file name. That search is a plain text search
inside the JSON. It does not read the graph and it does not resolve node inputs.

A workflow that only ever lived in a browser tab was never saved to disk, so
ComfyVault cannot see it at all. When there are no saved workflow files to
search, the app says exactly that instead of reporting every model as unused.

**It never moves weights inside `custom_nodes` or the Hugging Face cache.** They
are counted and reported, so the numbers add up. They are never moved. A custom
node loads its own weights straight from its own folder, and the Hugging Face
libraries manage their cache themselves.

**It only ever moves model weight files.** The scan takes files whose extension
is one of `.safetensors` `.ckpt` `.pt` `.pth` `.bin` `.gguf` `.onnx` `.pt2`
`.sft` `.pkl`, and whose size is over 1 MB. Both are settings you can change.

**It runs one long job at a time.** A scan, a consolidation, and an undo are
long jobs. A second one is refused while one runs, so two of them can never
touch the same files.

---

## How it avoids losing your models

Moving somebody's model weights is the kind of job where being mostly right is
worthless. These are the rules the engine holds, and each one is covered by
tests that fail if it breaks.

**Nothing moves until you press Apply.** The plan is a dry run. It is built by
reading the disk, and it changes nothing.

**A file is checked again immediately before it is touched.** The size and the
modification time are compared against what the scan recorded. If either
changed, that file is left alone and reported.

**Bytes are never deleted before the replacement is in place.** For each
duplicate, the engine renames the file aside, creates the link, and deletes the
renamed file last. If any step fails, the file is put back under its own name.
An interruption always leaves those bytes under one name or the other, never
under neither.

**A duplicate is re-read before it is deleted.** By default ComfyVault reads the
duplicate's bytes one more time, right before deleting it, and compares them
against the copy it kept. Deleting is the one thing this app does that cannot be
undone, so it is done on proof rather than on a hash from an earlier scan. You
can turn this off in Settings to go faster.

**A move to another drive is copy, verify, then delete.** The copy is written,
flushed to the disk, read back, hashed, and compared. Only then is the original
removed.

**Nothing is ever overwritten.** If something already sits where a file would
go, the engine refuses and reports it.

**Each model is all or nothing.** A model either finishes completely or leaves
the disk exactly as it was. A failure on one model never leaves another one half
done.

**Every step is journaled before it is performed.** The journal lives in the
vault. If the power goes out mid-run, the app shows you the interrupted run the
next time it starts, and finishes or undoes it. A resumed run finishes only the
models you originally ticked.

**Any run can be undone.** Undo puts every file back where it came from and
removes the link that stood in its place. Putting files back needs free space on
the drive they came from, and ComfyVault checks that first and refuses rather
than half doing it.

There is more detail, including what the vault folder holds and what happens
when a link breaks, in [docs/HOW-IT-WORKS.md](docs/HOW-IT-WORKS.md).

---

## The screens

**Home** is the report. How many installs, how many unique models, how much is
on disk, how much comes back. It shows the drive as it is, with the part this
run gives back marked on the meter.

**Library** is one row per unique model, whether its bytes are already in the
vault or still sitting in four installs. Filter it, sort it by how many places
hold a file, and see every name a model answers to.

**Consolidate** is the dry run and the Apply button. It is the screen this
product exists for.

**Cleanup** handles what goes wrong over time. Links that point at a file that
is not there are listed first and can be removed in one go. Files that arrived
under two different names are settled by picking which name the vault keeps, and
the other name stays as a link so saved workflows keep opening. Vault files that
nothing points at any more are listed last, with what deleting one costs.

**Settings** is your installs, your vault folder, and the scan rules. It also
re-checks Developer Mode and which ComfyUI processes are running.

**Download** is the one screen that stands empty in this version, on purpose. It
says what it will hold and what to do until then.

![Cleanup: broken links, duplicate names, and vault files nothing points at](docs/img/cleanup.png)

---

## What ComfyVault sends over the network

Nothing, in this version.

The engine can look a model up on Civitai by its hash. No screen calls that
today, so no hash of any model you own leaves your machine. There is no account,
no API key, and no telemetry.

---

## Where things live

The vault is a plain folder you choose. Inside it:

```
C:\ComfyVault\
  checkpoints\            one folder per model category
  loras\
  vae\
  .comfyvault\
    vault.redb            the record of what was taken and from where
```

Everything ComfyVault knows lives inside the vault, not beside the application.
Unplug the drive and plug it into another machine, and the record travels with
it. The only thing kept outside is which vault folder to open.

---

## Documentation

| Document | What it covers |
|---|---|
| [docs/HOW-IT-WORKS.md](docs/HOW-IT-WORKS.md) | What the scan reads, how the plan is decided, what Apply does step by step |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | Developer Mode, locked files, broken links, missing thumbnails, interrupted runs |
| [docs/BUILD.md](docs/BUILD.md) | Building on Windows, cross building from Linux, running the tests |
| [docs/IPC-CONTRACT.md](docs/IPC-CONTRACT.md) | Every command, payload, error and event between the window and the engine |
| [docs/FRONTEND.md](docs/FRONTEND.md) | Running the interface on its own, against a development engine |

---

## How it is built

The engine is a plain Rust crate, `crates/comfyvault-core`. It holds every rule
about models, vaults and links, and it knows nothing about the window it sits
in. The interface is Solid and TypeScript. Tauri puts the two together.

The split is deliberate. The same engine can back a command line tool later
without moving any logic.

Tests: 484 in the engine, 170 in the interface. The engine suite also runs on
Windows, which is the run that counts, because three real defects passed every
Linux test and failed on Windows. [docs/BUILD.md](docs/BUILD.md) has that recipe.
