# Troubleshooting

Most of what can go wrong is explained on screen, at the moment it happens, with
the fix next to it. This document covers the things you meet before the app can
explain them, and the ones that need a decision rather than a button.

---

## "No vault folder is open yet"

You tried to add an install before you chose a vault folder. The first-run
screen on Home asks for the vault first, but the **Add an install** button in
Settings is there before a vault exists.

1. Open Home, or the vault section of Settings.
2. Choose the vault folder.
3. Add the install again.

Everything ComfyVault knows lives inside the vault: which installs are
registered, what each scan found, and the journal of every run. So a vault has
to exist before an install can be recorded.

---

## A run stopped part way through

If the PC restarted or the app closed during a run, Consolidate shows **A run
stopped part way through** the next time ComfyVault opens. Nothing else can
start until you choose one of the two buttons:

- **Finish it** checks every file the run did not reach yet, and completes only
  the models you ticked for that run.
- **Undo it** puts back what the run already moved. It first shows what the
  undo costs.

Every step went into the journal before it happened, so each model is either
done or untouched, and every path holds its own file or a working link.

---

## An undo stopped part way through

You pressed **Stop now** during an undo, or the app closed during one. The
files already back stay back. The rest stay in the vault behind their links,
and they keep loading in ComfyUI.

Consolidate shows where the undo stopped. Press **Undo the rest** to finish it.

Scan again before you plan anything new. A scan from before the undo no longer
matches the disk, and ComfyVault says so.

---

## Apply is blocked

Two things on the PC block Apply. The Consolidate screen lists each one that is
true, with a re-check button.

### 1. Windows will not create links

Turn Developer Mode on. Open Settings, go to System, then For developers, and
turn Developer Mode on. On Windows 10 the page is under Update & Security. No
restart is needed.

Then press the re-check button in Settings. ComfyVault tests this by creating a
real link, reading it back, and deleting it, so the answer is a measurement, not
a guess.

If Developer Mode is already on and the check still fails, read the detail in
the panel. Group Policy can override the setting on a managed machine. Running
ComfyVault as an administrator also works, because an elevated process can
create links without Developer Mode.

### 2. ComfyUI is running

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

The message names the job that is running: a scan, a consolidation or an undo.
ComfyVault runs one long job at a time. Wait for the one running to finish, or
stop it.

Cancelling a scan changes nothing on disk, and the hashes already computed stay
in the cache, so starting again is not starting over.

---

## The first scan is slow

It reads every byte of every model file, because that is the only honest way to
know two files are identical. File size and date can agree while the contents
differ.

Later scans are much faster. Each hash is remembered against the file's path,
size and modification time, so a file that has not changed is not read again.

---

## Undoing a run

Open Consolidate and press **Undo this run**. Every file goes back to the path it
came from, and the link that stood in its place is removed.

An undo uses disk space, it does not free it. The copy the vault kept comes back
by a rename. Every duplicate the run deleted has to be copied back out of the
vault. The undo box shows the cost for each drive before anything starts. If a
drive does not have the room, the box says which one, and the undo does not
start. Free some space on that drive, then try again.

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

Use Node 22.13 or newer. The repository has an `.nvmrc`.

### The built application shows an empty window

The interface was not embedded. Build the interface first with `npm run build`,
then build the program with `--features custom-protocol`. Without that feature,
the program looks for the development server instead of the interface it
shipped with. [BUILD.md](BUILD.md) has the full recipe and how to check the
result.

### The window does not open at all

ComfyVault draws its window with Microsoft Edge WebView2. Windows 11 includes
it, and most Windows 10 PCs have it through Edge. If it is missing, install the
WebView2 Runtime from Microsoft and start ComfyVault again.

### `cargo build` for Linux fails

That is expected. The product targets Windows, and the desktop shell needs
libraries this project has no reason to install. The engine itself builds and
tests on Linux with nothing extra:

```
cargo test -p comfyvault-core
```

To build the Windows program, use the cross build in [BUILD.md](BUILD.md).

### Every test that creates a link fails

Developer Mode is off on the machine running the tests. That is the engine
reporting correctly, not a broken suite. Turn Developer Mode on and run them
again.
