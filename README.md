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

**Then clean up.** After a run, open the Cleanup screen. It shows what the run
could not settle by itself:

- **One model under more than one name.** The same file came in as, for
  example, `style.safetensors` in one install and `style_v2.safetensors` in
  another. The run already stored it in the vault under one of those names:
  the name of the copy it moved in. Your installs are not renamed: each one
  keeps its own file name, as a link to that vault file. Cleanup shows both
  names side by side, and you pick the name the vault keeps. The other name
  stays as a link, so saved workflows still open.
- **Vault files that nothing links to.** Delete these to free their space.
- **Links that point at a missing file.** Remove them.

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

1. Open [Releases](https://github.com/ruashots/ComfyVault/releases) and download
   `ComfyVault-v<version>-windows-x64.exe`.
2. Put it in any folder you like and run it. It is one portable program, with no
   installer and no install step.

**Windows warns you the first time.** The program is not signed yet, so
Microsoft Defender SmartScreen shows "Windows protected your PC". Click **More
info**, check that the publisher is shown as unknown and the file name is the
one you downloaded, then click **Run anyway**. Windows asks only once for that
file.

**Check the download, if you want to be sure it is the file this project
built.** Each release also has `SHA256SUMS.txt`. In PowerShell, in the folder
with both files:

```
Get-FileHash .\ComfyVault-v<version>-windows-x64.exe -Algorithm SHA256
Get-Content .\SHA256SUMS.txt
```

The two hashes must be the same.

Each program also carries a GitHub build attestation: a signed record that it
was built by this repository's release workflow, from the tagged source. With
the [GitHub CLI](https://cli.github.com/), version 2.49 or newer, installed:

```
gh attestation verify .\ComfyVault-v<version>-windows-x64.exe --repo ruashots/ComfyVault
```

It must end with a line saying the verification succeeded.

To build ComfyVault from source instead, follow
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
   duplicate copies. A model that sits under different names in different
   installs goes into the vault under the name of the copy it moves in. Every
   install keeps its own name for it, as a link. You can press **Stop now** at any time. Each model is done
   completely or not at all, and each path always holds its own file or a
   working link, so every model keeps loading in ComfyUI during the run.

6. **Clean up.** Open Cleanup and settle what the run left: pick one name for
   each model that arrived under several names, and delete the vault files
   that nothing links to.

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

**Library** lists the models in the vault, one row each. Before the first run
it is empty. Search it, sort it by how many installs link to a model, and see
every name a model has. Open a model to see what Civitai knows about it: its
name, base model, trigger words, and a link to its Civitai page. From there you
can also link the model into an install, in the folder you pick.

**Consolidate** shows the dry run and the Apply button, then the finished run
with its undo.

**Cleanup** handles what goes wrong later. Links that point at a missing file
come first, and you can remove them. A model your installs call by two names
gets a card: pick the name to use, and every install's link takes it. Cleanup
first lists the saved workflows that use the other name, because they show a
missing model until you pick the new name in them. The change can be undone,
and a card you hide stays hidden until a new name appears. Vault files that
nothing points at come next. Last, you can
delete a model the installs still use: the same delete removes every link to
it, in every install, and it cannot be undone.

**Download** takes the address of a model on Hugging Face or Civitai. It shows
the file, its size and its folder before anything is downloaded, then puts the
file in the vault and links it in the installs you tick, each in the folder you
pick, for example `loras\portraits`. The file is checked
against its SHA-256 before it goes into the vault. A download can be stopped
and continued, and one cut off by a closed app continues from where it
stopped.

**Settings** holds your installs, the vault folder, the scan rules, the
Civitai switch, and your Hugging Face and Civitai tokens. It also checks
Developer Mode again, and which ComfyUI programs are running.

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
and ComfyVault uses the network only for a download you start. Everything else
works the same without it.

**A download goes only to the site you pasted.** Some models download only for
a signed-in account. For those, paste your own Hugging Face token or Civitai
key in Settings. ComfyVault keeps each one in Windows Credential Manager on
this PC, never in the vault folder, and sends it only to the site it belongs
to.

There is no ComfyVault account and no telemetry.

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
