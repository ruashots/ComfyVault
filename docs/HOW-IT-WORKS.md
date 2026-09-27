# How ComfyVault works

This document is for the person who wants to know exactly what happens to their
model files before they let a program move them. It follows the work in order:
what the scan reads, how the plan is decided, what Apply does step by step, and
what the vault holds afterwards.

Every rule below is enforced by the engine in `crates/comfyvault-core` and
covered by its tests.

---

## 1. What a symbolic link is, and why this works

A symbolic link is an entry in a folder that points at a file somewhere else.
Windows has had them for years. To a program that opens the file, a link behaves
like the file: it has the same path, the same name, and reading it returns the
same bytes.

ComfyUI follows symbolic links when it lists and loads models. So a link left
where a model used to be is, from ComfyUI's point of view, the model.

This is why no workflow needs changing. The path did not move. Only the bytes
did.

Windows refuses to create a symbolic link for a program that is not elevated,
unless Developer Mode is on. That is the one system setting ComfyVault needs.

---

## 2. Registering an install

An install can only be registered once a vault folder is open, because the
record of every install lives inside the vault. The first-run screen asks for
the vault first for this reason.

ComfyVault has to be sure a folder really is a ComfyUI install before it reads
anything from it.

It looks for these seven entries and requires at least five of them in one
folder:

```
main.py   nodes.py   folder_paths.py   execution.py   server.py   comfy/   comfy_extras/
```

It then opens `folder_paths.py` and requires the text `folder_names_and_paths`
to appear in it. That content check removes false positives, for example a
backup folder that happens to hold a few of the same file names.

If the folder you chose fails the test, ComfyVault searches down to three levels
for a folder that passes, and offers the shallowest one it finds. This handles a
launcher layout, where the real install sits at something like
`C:\Something\ComfyUI-Easy-Install\ComfyUI\`.

It also reads the install's version, from `comfyui_version.py` and then from
`pyproject.toml`. ComfyUI only started recording its version in 0.3.11, so an
older install records it nowhere. A missing version is normal, and it never
stops an install being registered. It is reported as unknown, never as fine.

### `extra_model_paths.yaml`

If the install has one, ComfyVault reads it and follows the folders it names,
because ComfyUI does. The reader reproduces ComfyUI's own parsing rules,
including the parts people get wrong:

- A tilde expands on `base_path` only. A `~` written in a category value stays a
  literal folder named `~`.
- An absolute category path discards `base_path` entirely.
- ComfyUI renames two categories on the way in. `unet` becomes
  `diffusion_models`, and `clip` becomes `text_encoders`. ComfyVault applies the
  same renaming and reports both names.

A category name in that file becomes a folder name inside the vault, so
ComfyVault checks that it is a single safe folder name. A category that is not
one is skipped, and the reason appears in the install's details, so a broken
hand edit shows up as a message rather than as a silently missing folder.

---

## 3. The scan

A scan walks these places, for every install in the scan:

| Place | What happens to it |
|---|---|
| `<root>\models` and everything under it | Read, and movable |
| Every folder in `extra_model_paths.yaml` | Read, and movable |
| `<root>\output\` for `checkpoints`, `clip`, `vae`, `diffusion_models`, `loras` | Read, and movable. ComfyUI registers those five as model search paths at startup |
| `<root>\custom_nodes` and everything under it | Counted, **never moved** |
| The Hugging Face cache | Counted, **never moved** |

A file enters the scan when its extension is in the list and its size is over
the floor. The defaults are:

```
.safetensors  .ckpt  .pt  .pth  .bin  .gguf  .onnx  .pt2  .sft  .pkl
```

and 1 MB. Settings shows both rules. This version has no control to change
them.

The first seven extensions are the common weight formats. The last three come
from ComfyUI itself, which treats `.pt2`, `.sft` and `.pkl` as model weights.

### Hashing

ComfyVault computes a SHA-256 hash of the whole file. Two files are duplicates
when their hashes match, whatever they are called and wherever they sit.

A first scan therefore reads every byte of every model. On a terabyte of weights
that takes a while. There is no shortcut that is also honest: file size and date
can agree while the contents differ.

Later scans are much faster. Each hash is cached against the file's path, its
size, and its modification time. If all three match, the cached hash is reused
and no bytes are read. Change any of the three and the file is read again.

### Links the scan meets

The scan follows symbolic links while it walks, because ComfyUI does, and the
two must agree about which files exist. One physical file reached by two routes
is counted once.

A file that is already a link into the vault is recorded as already done. So is
a file whose real place is inside the vault, however the scan reached it, for
example through an `extra_model_paths.yaml` entry that names a vault folder. A
vault file is never taken for a copy of itself. A link that points anywhere else is left alone: ComfyVault does not replace a link
somebody else made.

A file that is reached through a linked **folder**, and that really lives
outside every folder the install declares, is not treated as movable either. It
is reported with the path it actually lives at. A junction pointing at another
drive is a normal thing to find on a Windows machine, and a plan must never move
a file out of a folder it never named.

---

## 4. The plan

The plan is a dry run. Building it changes nothing on disk.

**One group per unique content.** Every file with the same SHA-256 becomes one
group, whatever it is called and whatever folder it sits in. The same weight
under `loras\awesome\` in one install and `loras\new\` in another is one group.

**The vault path is the category plus the file name.** So
`loras\awesome\lora1.safetensors` becomes `loras\lora1.safetensors`. The
sub-folder disappears inside the vault. It stays in the install, because the
link stays where the file was.

**Which copy moves.** ComfyVault prefers a copy that already sits on the vault's
own drive, because moving that one is a rename: it takes no time and no extra
space. If no copy is on that drive, it takes the first copy by sorted path. The
plan says which rule decided, for every group.

**Every place gets a link, including the one that moved.** A group that covers
four paths creates four links.

**Two different files with the same name both survive.** The one held in more
places keeps the plain name. The other takes its name plus the first eight
characters of its hash, for example `lora1__3F9A2C17.safetensors`. Ties are
broken by hash, so a plan built twice from one scan is identical.

**A model the vault already holds moves nothing in.** This happens on a later
run, when a new copy of a model turns up after an earlier run put that model in
the vault. Every copy, the one listed as the source included, becomes a link to
the existing vault file, and its bytes are deleted. Before any copy is touched,
ComfyVault reads the vault file and proves it is the same model, because it
becomes the only copy. If the vault file is missing, or is not a real file of
the right size, the copies move in as for a new model.

**A model that exists only once still moves into the vault.** It frees nothing,
and the plan counts it separately, because the space number has to match the row
count you are looking at.

**A file that cannot move never appears in a group.** It appears in its own list
with a reason and a sentence explaining what to do. The reasons include: another
program has the file open, the file changed after the scan read it, the file is
gone, permission was refused, the vault folder sits inside that install, there
is not enough room on the vault's drive, and the name is already taken inside
the vault by different content.

### The plan is rebuilt, never patched

Building a plan re-checks the machine. It asks again whether each file still
matches what the scan read, and whether another program holds it open.

So the plan changes as the machine changes, and it changes in the direction
people expect. Close ComfyUI and build the plan again from the same scan, and
the plan gets **bigger**: a file that was held open rejoins its group, and a
content that looked like a single copy becomes a duplicate that saves space.

---

## 5. Apply, step by step

Apply is sent the exact list of groups you left ticked. It reads the stored
plan. It does not re-plan, and it does not widen the work.

For each group:

1. **Check every file again.** The size and the modification time are compared
   against what the scan recorded. If either differs, that group stops, the
   reason is recorded, and the run moves to the next group. On Windows the
   engine also checks whether another program holds the file open, by opening it
   the same way the move would.

2. **Move the chosen copy into the vault.** On the same drive this is a rename,
   which is instant. Across drives it is a copy: the bytes are written, flushed
   to the disk, read back, hashed, and compared against the hash the scan
   recorded. Only then is the original removed. A mismatch deletes the copy and
   leaves the original untouched.

3. **Put a link where that copy was.** Its old place is now empty, so it only
   needs the link.

4. **For every other copy: rename aside, link, verify, delete.** The file is
   renamed to a temporary name next to itself. The link is created. The
   duplicate's bytes are read one more time and hashed, and the hash is compared
   against the copy being kept. Only if they match is the renamed file deleted.
   If they do not match, the file is put back and the group is reported.

That last re-read is the setting `verifyBeforeDelete`, and it is on by default.
Deleting is the only step in the whole program that cannot be undone. Without
the re-read, the proof that two files are identical is a hash from an earlier
scan, which may itself have come from a cache row rather than from the file.
Drives with coarse timestamps, which external model drives often have, can hide
a difference from the size and the date alone.

You can turn it off to go faster. That makes the delete a matter of trust rather
than proof.

### Stopping a run

**Stop now** on the running screen stops the run as soon as it can. A model
that was part way through is put back first, so each model is done completely
or not at all. Everything already done stays done, and the run can still be
undone.

During the whole run, each path holds either its own file or a working link.
So every model keeps loading in ComfyUI, even while a model is part way
through, and even if the run is stopped.

A model the run stopped on is not reported as a failure, because you stopped
it. A model that failed on its own earlier in the same run is still reported.

### Two rules that hold throughout

**Each group is all or nothing.** If any step in a group fails, every step
already done in that group is undone, and the disk is left exactly as it was. A
failure in one group never leaves another group half finished.

**Nothing is ever overwritten.** If anything already sits where a file or a link
would go, the engine refuses and reports it. This holds for the move, the link,
the put-back, and the undo.

---

## 6. The journal, and what happens after a crash

Before each step runs, ComfyVault writes what it is about to do into a journal
in the vault database. The write reaches the disk before the step happens.

So the journal is always at least as far along as the disk. If the power goes
out, the app can read the journal, look at the disk, and see exactly where it
stopped.

The next time ComfyVault starts, it lists any run that did not finish and asks
you to resolve it before anything else. You can:

- **Finish it.** It re-checks every file it had not yet touched, and completes
  only the groups you originally ticked. It never widens the work to the rest of
  the plan.
- **Undo it.** Every file goes back to where it came from.

Because an interrupted duplicate is always under one name or the other, the
renamed-aside name or its original name, its bytes are never in neither place.
The model's path is a different matter. A crash between moving a model into the
vault and making its link leaves that path empty, so that one model does not
load until the run is finished or undone.

### A run ComfyVault will not touch

Before it finishes or undoes a cut-off run, ComfyVault checks every place the
run names. Each one must be inside the vault, or inside a registered install
that is a ComfyUI install on this computer now. Each file must also have a
model file's name. If any place fails, ComfyVault refuses both Finish and Undo,
and names the places.

This happens when the vault was opened on a different computer, or an install
was moved or removed after the run. A vault someone else prepared can cause it
too, so look at the places before you go on.

The one way forward is **Set it aside**. It changes only the run's record, and
nothing on the disk moves. The links the run made keep pointing into the vault.
A model the run was in the middle of can stay without a file or a link at its
path. The run stops blocking the app, and a new scan is needed before the next
plan.

Setting a run aside is not final. When its places pass the check again, for
example when an unplugged drive is back, the run comes back as a cut-off run,
and you can finish it or undo it.

---

## 7. Undo

Undo walks the journal backwards and reverses every step.

### What it costs, shown before it starts

Before an undo starts, ComfyVault shows how many files come back by a rename,
how many have to be copied, how much data the copies write, and how much room
each drive is expected to need beside the room it has free. If a drive is
short, the undo does not start.

### How the files come back

The copy that was kept comes back by a rename. Its bytes are the vault file, so
on one drive this is instant and takes no room. If the vault is on a different
drive from the install, this copy is copied back as well.

Each duplicate comes back as a copy of the vault file. Its own bytes were
deleted, which is what freed the room, and one file cannot be renamed into two
places. The copies take time, and they take room on the drive the files go back
to. ComfyVault checks that room first and refuses rather than half doing it.

A copy keeps what the drive knew about the file. A sparse file stays sparse, an
NTFS compressed file stays compressed, and each file gets back the modification
time it had before the run. The room an undo takes is therefore what the files
really occupied, which for a sparse file can be almost nothing.

### Stopping an undo

An undo can be stopped at any moment with **Stop now**, and ComfyUI still loads
every model. Each file comes back by one rename onto the link that stood in its
place. A copy is written to a temporary file beside the link, checked, and then
renamed over it. So a model's path always holds the link or the file, even if
the power goes.

A stopped undo leaves the run partly undone. The files already back stay back,
and the rest stay in the vault behind their links. Consolidate then shows where
the undo stopped, with **Undo the rest** to finish it. An undo cut off by a
crash is finished the same way.

A run that linked copies to a vault file from an earlier run puts those copies
back, and leaves the vault file, because the earlier run owns it.

A run that was interrupted and then finished is undone as one run. Resuming
continues the same journal rather than starting a new one, so the whole of it
comes back.

An undo is refused when something you did later still depends on the run. The
common cases are renaming a model inside the vault, and a later run that linked
new copies to a vault file this run put there. ComfyVault names what is in the
way, so you can undo the later change first.

---

## 8. The vault

The vault is a plain folder. You choose where it goes. It cannot be inside a
ComfyUI install, and ComfyVault refuses that choice, because a file moved into
it would still be inside the install it came from.

There is no default vault folder. The folder you choose must be empty, or not
exist yet, or already be a ComfyVault vault, which opens as it is. A folder that
holds other files is refused, and nothing in it changes. Its files would
otherwise be taken for vault files. **New folder** in the folder picker makes an
empty folder inside the one you picked.

### Which drive

**Put the vault on the same drive as your installs.** There, a file is moved by
a rename. The move is instant, and the vault needs no free space of its own. On
any other drive, every file is copied across and checked first, so that drive
needs the room before the run.

**Do not put it on a removable drive or a network drive.** Every install points
into the vault by link. On any day that drive is missing, every model in every
install stops loading at once. ComfyVault warns you about this kind of drive,
and the choice stays yours. A drive that does not answer at all is refused.

**A vault on a different drive from an install is not proven.** The copy across
drives, and the undo of it, have only been tested against a simulated drive
boundary, never a real second drive. If you choose one, keep your own backup of
those models until you have checked a run and an undo yourself.

A vault made at `C:\ComfyVault`, for example:

```
C:\ComfyVault\
  checkpoints\
  loras\
  vae\
  .comfyvault\
    vault.redb
```

One folder per model category, and one real file per unique content.

### Names

The same file often arrives under two different names, because you downloaded it
twice from different places. The vault keeps one of them as the real file, and
the other as a link beside it. So every name a model was ever known by still
works, and a saved workflow that names the old one still opens.

The Cleanup screen shows these, and lets you pick which name the vault keeps.
Doing that frees no disk space. What it gives you is one entry per model in
ComfyUI's dropdown instead of two. Removing a name is a separate action, and it
is refused while any install link still resolves through it.

### The record

`vault.redb` holds the whole record: which installs are registered, what each
scan found, every plan, every run's journal, every link that was created, and
the cached hashes.

It lives inside the vault, not beside the application. Move the drive to another
machine and the record of what was taken and from where moves with it. The only
thing kept outside the vault is which vault folder to open.

---

## 9. What can go wrong later, and how the app finds it

The Cleanup screen runs a health check over the vault and the links.

**A link that points at a file that is not there** is the most serious result,
and it is shown first. It happens when a vault file is deleted or the vault
drive is unplugged. It matters for a reason that is not obvious: ComfyUI lists a
broken link in its model dropdown and then fails to load it, and a custom node
that re-downloads what it thinks is a missing model writes straight through the
broken link and drops the file inside the vault. ComfyVault offers to remove
broken links, singly or all at once. No model file is lost by removing one, since
a broken link points at nothing.

**A real file where a link belonged** means something replaced the link.
ComfyVault reports it and does not touch it.

**A vault file nothing points at** is listed in Cleanup. Deleting one does free
space, and it cannot be undone.

**A model that installs still link to** can be deleted from Cleanup as well.
The same delete removes every link to it, in every install, so the model
disappears from every ComfyUI that used it. It cannot be undone.

ComfyVault checks everything first. Each link must still be a link to that
file, each second name in the vault must still be a link beside it, the file
must still be the size the vault recorded, and no other program may hold it
open. If any check fails, nothing is removed, and the paths are named.

The links go first, then the second names, then the file. If Windows refuses
any of those, ComfyVault puts back the links it already removed, and the model
loads as before. An undo of the run that brought the file into the vault is
refused afterwards, because the file it needs is gone. The refusal says that
one of the run's models was deleted in Cleanup, and nothing is changed. A run
made after the delete can still be undone.

While a model is being deleted, nothing else can make or remove a link. Each
link is checked again right before it is removed, so a real file that took its
place is never deleted.

If the computer stops part way through a delete, some installs can lose their
link while the model is still in the vault. The health check in Cleanup lists
the model, and deleting it again finishes the delete.

**A file in the vault folder that the record does not know about** is reported
as well, rather than quietly adopted.

---

## 10. Is a model used

The Library can search your saved workflow files for a model's file name.

Read what this check actually does, because the answer is easy to misread:

> The file name is searched for as plain text inside saved workflow files.

It does not parse the graph. It does not resolve node inputs. A match means the
name appears in a file. It does not prove the model runs.

It searches these places under each install root:

```
every .json file under user\, up to eight folders deep
any file named workflow.json, up to four folders deep, outside user, models,
  custom_nodes, output, input, temp, .git, venv, .venv and python_embeded
```

The `user` folder holds saved workflows and subgraphs. It also holds other JSON
files, such as settings, so a name found there does not always come from a
workflow.

Files over 50 MB are skipped and reported.

A workflow only exists on disk once you pressed Save. A draft that lived in a
browser tab is invisible to this check. When there is nothing on disk to search,
ComfyVault says exactly that instead of reporting every model as unused:

> No saved workflow files were found, so nothing was searched. A workflow that
> was never saved lives in the browser, where this app cannot see it.

**This is not a safe-to-delete list.** Treat it as a hint about which models to
look at, and never as permission to delete one.

---

## 11. Model thumbnails

ComfyUI 0.28.0 added a security check to the route that serves model preview
images. That check rejects a file reached through a per-file symbolic link.

The effect is limited to thumbnails in the model browser. Loading a model is not
affected. Running a workflow is not affected.

This will not be reverted, and it is the price of per-file links. ComfyVault
reads each install's version and tells you which of your installs it affects. An
install that does not record its version is reported as unknown, never as
unaffected, because the app cannot check what the install does not say.

---

## 12. What Civitai tells you about a model

After each scan, ComfyVault asks Civitai about the model files it has not asked
about before, in the background, a hundred at a time. It sends each file's
SHA-256 hash. It sends no file name and no path. Civitai answers with the
model's name, version, base model, trigger words and the link to its page, and
the Library shows them when you open a model.

The lookup is on by default. The **Civitai lookup** switch in Settings turns it
off, and then ComfyVault uses the network only for a download you start.

- A file Civitai does not know stays without a label. That is normal. The
  answer is remembered, so the file is not asked about again.
- If Civitai cannot be reached, Settings says so, and ComfyVault asks again
  after the next scan. A scan, a plan and a run never wait for a lookup.

---

## 13. Downloading a model

The Download screen takes the address of a model on Hugging Face or Civitai.
For Hugging Face, it is the address of one file, from that file's page. For
Civitai, it is a model page, with or without a version.

### Reading the address

ComfyVault asks the site what the address names: the file, its size, and its
SHA-256 when the site states it. Civitai states it for every file. Hugging
Face states it for a large file, and not for a small one stored without LFS.

It also asks for the file itself, with your token if you saved one, and
reads the answer without following it. So a model that needs an account, or
that your account has no access to, is refused here, before anything is
downloaded. The site's own words are shown as it wrote them.

### Where it goes

The file goes into the vault, in the folder you choose. ComfyVault suggests
one: from Civitai's kind of model, or from a folder name in the Hugging Face
path.

Each install you tick gets a link, in the folder you pick for it. Click the
path to pick another. The choice offers every folder ComfyUI reads for that
kind of model in that install, and every folder inside them: `models\{folder}`,
the older names ComfyUI still reads, such as `models\unet`, and the folders the
install's `extra_model_paths.yaml` adds, on any drive. Nothing outside them is
offered or accepted, because a link there would not show in ComfyUI. This
follows ComfyUI's own code.

The folder offered first is the one you picked last time for that kind in
that install. The first time, it is the folder where ComfyUI saves new files
of that kind: `models\{folder}`, unless `extra_model_paths.yaml` marks
another folder `is_default` for it. A folder you make in the choice is made
only when the link is.

ComfyUI names a model in a folder inside, for example
`loras\portraits\x.safetensors`, as `portraits\x.safetensors`. An install where a
different file already has that name, in any folder ComfyUI reads for that
kind, gets no link. Only one of the two files would load, so ComfyVault
leaves that install alone.

The Library's **Link into an install** offers the same choice of folders.
There too, each step is written to the journal first, so a link cut off by a
crash before its record was saved gets its record when the vault next opens.

If the vault already holds the same file, nothing is downloaded. The installs
you tick get links to the file that is there.

### The transfer

One download runs at a time. The bytes go into a part file inside the
vault's own `.comfyvault` folder, on the vault's drive, so moving it into
place at the end is a rename. ComfyVault needs the file's size plus 5 GB free
on that drive.

Both sites send the download on to a storage address that is signed and
expires. ComfyVault asks the site again every time a download starts or
continues, and never reuses an old storage address. Your token goes only to
the site, never to the storage address.

A stopped, failed or cut-off download keeps its part. Continue asks for the
rest only, and names the version the part came from. If the file changed on
the site since, the storage sends the whole file, and the old part is
dropped rather than joined to the new one.

Nothing is tried again without you. A dropped line, a line silent for 30
seconds, an expired storage address, a refusal part way and a full drive
each stop the download and say what happened.

### Before anything is linked

The whole file is hashed. If it is not the file the site named, it is
deleted, nothing goes into the vault, and nothing is linked. With no SHA-256
from the site, the hash is used to check whether the vault already holds the
file.

Then the file is renamed into the vault, never over anything, and each ticked
install gets its link. Each step is written to the journal first. If the app
closes after the file went into the vault, Continue finds it, checks it, and
makes the record and the links.

### Your tokens

Each token is kept in Windows Credential Manager, under your Windows account
on this PC. It is not in the vault folder, so it does not travel with the
vault. ComfyVault checks a token with the site before it saves it, and never
shows it again.

