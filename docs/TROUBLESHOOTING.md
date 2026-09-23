# Troubleshooting

Most of what can go wrong is explained on screen, at the moment it happens, with
the fix next to it. This document covers the things you meet before the app can
explain them, and the ones that need a decision rather than a button.

---

## "No vault folder is open yet"

Choose the vault folder before you add an install. Open Settings and pick it.

Everything ComfyVault knows lives inside the vault: which installs are
registered, what each scan found, and the journal of every run. So a vault has
to be open before an install can be added.

---

## Apply is greyed out

Three things block Apply. The Consolidate screen lists whichever ones apply, in
this order.

### 1. A run stopped part way

A previous consolidation did not finish, probably because the machine restarted
or the app was killed. Nothing else can run until it is resolved.

Finish it or undo it. Both are offered. Finishing re-checks every file it had
not yet touched, and completes only the models you originally ticked.

### 2. Windows will not create links

Turn Developer Mode on. Open Settings, go to System, then For developers, and
turn Developer Mode on. No restart is needed.

Then press the re-check button in Settings. ComfyVault tests this by creating a
real link, reading it back, and deleting it, so the answer is a measurement, not
a guess.

If Developer Mode is already on and the check still fails, read the detail in
the panel. Group Policy can override the setting on a managed machine. Running
ComfyVault as an administrator also works, because an elevated process can
create links without Developer Mode.

### 3. ComfyUI is running

Windows refuses to move a file that another program holds open. ComfyVault finds
ComfyUI processes by looking at what each running program is, where it is
running from, and what it was started with.

Close every ComfyUI window, including a server running in a terminal, then press
the re-check button.

---

## The plan got bigger after I closed ComfyUI

That is correct, and it is worth understanding.

A plan is built fresh every time. It re-checks whether each file still matches
what the scan read, and whether another program holds it open. A file that was
held open is left out of its group. Leave it out and a model that exists three
times looks like it exists twice, or like a single copy that saves nothing.

Close ComfyUI, build the plan again from the same scan, and those files rejoin
their groups. The plan grows and the space it returns grows with it.

So: **close ComfyUI before you scan, and before you apply.** You will get the
real number.

---

## Files that stay put

The plan lists every file it will not move, with a reason. The ones you can act
on:

| What it says | What to do |
|---|---|
| The file is open | Close ComfyUI, or whatever else is using it, and build the plan again |
| Changed since the scan | Something wrote to the file after the scan read it. Scan again |
| No longer there | Something moved or deleted it. Scan again |
| Permission refused | Windows will not let ComfyVault write in that folder. Check the folder's permissions |
| The vault name is taken | Something already sits at the name this file would take in the vault, and it is not a link. ComfyVault never overwrites. Rename one of them and scan again |
| Not enough room | The vault's drive cannot take the file. Free space, or put the vault on the same drive as the install |
| Its folder name escapes the vault | A category name in that install's `extra_model_paths.yaml` is not a valid folder name. Fix the name in that file and scan again |

Two reasons are not problems and need nothing from you. Weights inside
`custom_nodes` are loaded by a node straight from its own folder, so they are
counted and left alone. Weights in the Hugging Face cache are managed by the
Hugging Face libraries themselves, so they are counted and left alone too.

---

## ComfyUI stopped showing preview pictures for my models

This is expected, from ComfyUI 0.28.0 onward, and it cannot be worked around.

ComfyUI 0.28.0 added a security check to the route that serves model preview
images. The check rejects a file reached through a per-file symbolic link. It
will not be reverted.

Loading the model still works. Running the workflow still works. Only the small
preview picture in the model browser is gone.

ComfyVault tells you which of your installs this affects before you apply
anything. An install that does not record its version is reported as unknown,
because ComfyUI only started recording the version in 0.3.11, and an unknown
version cannot be called safe.

If the pictures matter more to you than the disk space, undo the run. The undo
puts every file back.

---

## ComfyUI lists a model and then fails to load it

A link is pointing at a file that is not there.

Open Cleanup. Broken links are listed first. Remove them, singly or all at once.
No model file is lost by removing one, because a broken link points at nothing.

This happens when a vault file was deleted, or when the vault lives on a drive
that is not plugged in.

Fix the cause first if you can. If the vault drive is simply unplugged, plug it
in rather than removing the links.

There is a second reason this matters. A custom node that re-downloads what it
thinks is a missing model writes straight through the broken link, and the file
lands inside the vault where nothing is expecting it. Clearing broken links
early avoids that.

---

## The app opened with no vault

ComfyVault remembers the vault folder and opens it at startup. If that folder is
gone, or its drive is not plugged in, the window still opens and tells you what
happened, rather than refusing to appear.

Plug the drive in and choose the vault again in Settings, or choose a different
one.

---

## "This vault was made by a newer version of the app"

The vault's record was written by a build that understands more than this one
does. Update ComfyVault.

Opening it with the older build is refused on purpose. Writing an old shape into
a newer record is how a record gets corrupted.

---

## "The vault database could not be opened"

Another copy of ComfyVault has it open. Close the other window.

Two copies of the app are kept apart by a lock on the vault database, so they
can never work on the same files at once.

---

## "A scan is already running"

ComfyVault runs one long job at a time. A scan, a consolidation, and an undo are
long jobs. Wait for the one running to finish, or cancel it.

Cancelling a scan changes nothing on disk, and the hashes already computed stay
in the cache, so starting again is not starting over.

---

## The first scan is slow

It reads every byte of every model file, because that is the only honest way to
know two files are identical. File size and date can agree while the contents
differ.

Later scans are much faster. Each hash is remembered against the file's path,
size and modification time, so a file that has not changed is not read again.

If you want to force a full read, turn the hash cache off in Settings.

---

## I want the space back

Open the run in Consolidate and press undo. Every file goes back to the path it
came from, and the link that stood in its place is removed.

Putting files back needs free space on the drive they came from. ComfyVault
checks that first and refuses rather than half doing it.

An undo is refused if something you did later depends on the run. The usual case
is renaming a model inside the vault. ComfyVault names what is in the way, so
you can undo that first.

---

## Removing an install from the list

Removing an install from ComfyVault deletes nothing and removes no link. Every
link inside that install is left exactly as it is, so the install keeps working
and everything it loads keeps loading.

The vault files those links point at are still needed. Do not delete the vault
after removing an install from the list.

Vault files that nothing points at any more appear in Cleanup, where deleting
one asks you to type its hash back, and says plainly that it cannot be undone.

---

## Building

### Node 20 cannot install the dependencies

Use Node 22 or newer. The repository has an `.nvmrc`.

### The built application shows an empty window

The interface was not embedded. Build the interface first with `npm run build`,
and build the application with `npm run tauri build` rather than a hand-rolled
`cargo build`.

A direct `cargo build` needs `--features custom-protocol`. Without it the
application looks for the development server instead of the interface it
shipped with. [docs/BUILD.md](BUILD.md) has the full recipe and how to check the
result.

### `cargo build` for Linux fails

That is expected. The product targets Windows, and the desktop shell needs
libraries this project has no reason to install. The engine itself builds and
tests on Linux with nothing extra:

```
cargo test -p comfyvault-core
```

To build a Windows application from Linux, use the cross build in
[docs/BUILD.md](BUILD.md).

### Every test that creates a link fails

Developer Mode is off on the machine running the tests. That is the engine
reporting correctly, not a broken suite. Turn Developer Mode on and run them
again.
