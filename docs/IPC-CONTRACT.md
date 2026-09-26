# ComfyVault IPC contract

This document defines every Tauri command, every payload, every error, and every
event. The user interface builds against this document. The Rust engine
implements it.

Version 1. Status: stable. A change to this document is announced before it
lands.

---

## 1. Conventions

### 1.1 Naming

- Command names are `snake_case`.
- All JSON field names are `camelCase`, in both directions.
- Each command takes exactly one argument object named `args`.
- A command with no input takes no argument.

Call example:

```ts
import { invoke } from '@tauri-apps/api/core'

const installs = await invoke<Install[]>('list_installs')
const info = await invoke<InstallCandidate>('validate_install_path', {
  args: { path: 'C:\\SomeFolder' }
})
```

### 1.2 Types used everywhere

| Type | Encoding |
|---|---|
| Timestamp | RFC 3339 string in UTC, for example `2026-09-22T14:31:07.482Z` |
| Byte count | JSON number, always an integer, always bytes |
| File path | String. On Windows the separator is a backslash, and no path ever carries the `\\?\` prefix. A path sent to the engine may use either separator. |
| Identifier | String. Format is UUID v4 unless this document says otherwise. |
| Hash | 64 hexadecimal characters, uppercase. The algorithm is SHA-256. |

### 1.3 Samples of every payload

`docs/golden/` holds one JSON file per payload, named after the payload. Each
file is real output. The engine writes it with the same serialiser the product
uses, and a test fails if a file stops matching what the engine sends.

Build test doubles from those files. Do not restate a shape by hand. A hand
written double agrees with whoever wrote it, which is how the engine came to
send `huggingfaceCacheDirs` while the interface read `huggingFaceCacheDirs`:
both sides were green and three panels of Settings were empty on every machine.

This document says what a field means, what is allowed, and what the engine
promises. The sample says what arrives. If the two ever disagree, the sample is
right, and the disagreement is a bug to report rather than to work around. A
test compares this document's field lists against the samples, so that gap
should not last.

To accept a deliberate change to the engine's output:

```
UPDATE_GOLDEN=1 cargo test -p comfyvault-core golden
```

#### What a sample cannot tell you

A sample proves the **names, the shapes and the kinds** of a payload. Its
**values** are written by hand, so for a field whose value the operating system
produces, the sample shows what somebody typed rather than what will arrive.

This has cost real work once. `volume` was written `C:` and Windows returns
`C:\`. Both are strings with the right name, so a check on names passed and a
check on kinds passed, and an interface built against the sample told people
their files would be copied when they would be renamed.

The fields whose value comes from the operating system rather than from the
engine's own records:

| Field | Held to a real answer? |
|---|---|
| `VaultInfo.volume` | yes, the shape is checked against this machine |
| `DriveInfo.root`, `DriveInfo.kind` | yes, same |
| `PlatformReport.os`, `PlatformReport.symlinks` | yes, the name must be one the engine sends |
| `LockState.*`, `RunningComfy.*` | **no** |
| `mtimeNanos` anywhere | **no** |

The rows marked no have no fixed shape to hold them to, so their samples are
illustrative. Do not read a value from one of those and compare it with
anything. Ask the engine instead.

### 1.4 Errors

Every command rejects with the same shape:

```ts
type VaultError = {
  code: ErrorCode
  message: string      // one sentence, safe to show to a person
  detail?: string      // technical text, safe to show in a details panel
  path?: string        // the path that caused the failure, if there is one
}
```

`message` is always written for a person to read. `detail` is always written
for a developer to read. The engine never puts a raw stack trace in either
field. The engine never puts a secret in either field.

`ErrorCode` is one of:

| Code | Meaning |
|---|---|
| `notInitialized` | No vault is open. Call `select_vault` first. |
| `vaultBusy` | A scan, an apply, or a revert already runs. |
| `invalidArgument` | An argument failed validation. Read `detail`. |
| `notFound` | The identifier does not exist. |
| `pathOutsideBoundary` | A path escapes the vault or the install that owns it. |
| `notAComfyInstall` | The folder is not a ComfyUI install. |
| `alreadyRegistered` | The install is already registered. |
| `ioError` | The file system refused an operation. |
| `permissionDenied` | The operating system refused access. |
| `fileLocked` | Another program holds the file open. |
| `fileChanged` | The file changed after the scan read it. |
| `symlinkUnsupported` | This system cannot create symbolic links. |
| `storeError` | The vault database refused an operation. |
| `parseError` | A configuration file is malformed. Read `path`. |
| `networkUnavailable` | A metadata lookup failed. The lookup is optional. |
| `cancelled` | The caller cancelled the operation. |
| `conflict` | The operation contradicts the current state. Read `detail`. |

### 1.5 Before a vault is chosen

A new person starts here, every time, and it is the state the window opens in.
**Almost every command refuses until a vault is open.** They answer
`notInitialized` with the sentence "No vault folder is open yet. Choose a vault
folder to continue."

These are the only calls that answer in that state:

| Command | What it gives you |
|---|---|
| `get_app_state` | `vaultInitialized: false`, `vaultRoot: null`, `installCount: 0`, the real `platform`, and `settings` at their defaults |
| `get_platform_report` | what this computer can do |
| `validate_install_path` | so a folder can be checked before there is anywhere to record it |
| `check_locked_files` | it asks the operating system, not the vault |
| `list_drives` | every drive on this computer, with its size and its free space |
| `open_task_manager` | it opens a Windows program, and reads nothing in the vault |
| `open_civitai_page` | it opens a web page, and reads nothing in the vault |
| `open_huggingface_page` | the same |
| `set_token`, `get_token_status`, `remove_token` | a token lives in Windows Credential Manager, not in the vault |

`select_vault` is the way out of the state, and it works.

Everything else refuses, `list_installs`, `get_last_scan`, `list_applies`,
`get_interrupted_applies` and `get_running_comfy` included. Those five look
harmless and are not: each reads something that lives inside the vault.

**Draw the first screen from `get_app_state` alone.** It carries the platform
and the default settings for exactly this reason, so that nothing else has to
be asked. A start up sequence that fetches a handful of lists in parallel
before checking `vaultInitialized` gets a refusal on the first frame, and the
person meets an error instead of the screen that asks for a folder.

A refusal here is not a failure. It is the engine saying the person has not
chosen a folder yet, which on first run is simply true.

#### The order a first run must follow

`select_vault` first, then everything else. There is no way around it, and the
engine will not do it for you.

1. `select_vault` with `{ path, createIfMissing: true }`. This is what makes
   the vault: it creates the folder if it is not there, and creates the
   database inside it. It answers with `VaultInfo`.
2. `register_install` for the person's first ComfyUI folder.

With `createIfMissing: false` and nothing at that path, `select_vault` rejects
with `notFound` and creates nothing. Pass `true` on a first run.

`register_install` before `select_vault` rejects with `notInitialized`, like
every other command that reads or writes the vault. There is nowhere to record
an install until a vault exists.

**There is no default vault folder.** The engine never opens one on its own,
and the interface suggests none. On a machine
where a vault was opened before, the engine reopens that one at startup, from
the path it recorded. On a machine where one never was, nothing is open and
nothing is created until `select_vault` is called.

That is deliberate. The vault is a folder that will end up holding hundreds of
gigabytes of the person's files, so it gets made because the person agreed to
make it, not as a side effect of a button that says something else.
`select_vault` is also where the engine refuses a folder inside a ComfyUI
install, and that refusal is worth nothing if a vault can appear without it.

### 1.6 Concurrency rule

The engine runs one long operation at a time. A scan, an apply, and a revert
are long operations. If a second long operation starts, the engine rejects it
with `vaultBusy`. All other commands stay callable during a long operation,
except `delete_vault_file` with `removeLinks: true`, which section 8.8
describes. While that delete runs, the commands that change a link or a vault
name wait for it to finish. It removes links and one file, so the wait is
short.

A download is not a long operation. It runs beside a scan, a consolidation
and an undo, and section 16.3 says how they meet.

---

## 2. Platform and application state

### 2.1 `get_platform_report`

Reports what this computer can actually do. The engine measures the answer. It
does not guess the answer from the operating system name.

Arguments: none.

Returns:

```ts
type PlatformReport = {
  os: 'windows' | 'linux' | 'macos'
  symlinks: {
    supported: boolean          // the engine created a test symlink and deleted it
    probeError: string | null   // why the test failed, if it failed
    developerMode: boolean | null  // Windows only. null on other systems.
    elevated: boolean           // the process runs with administrator rights
    guidance: string | null     // one sentence to show the person
  }
  longPathsEnabled: boolean | null  // Windows only. null on other systems.
}
```

How the engine decides `supported`: it creates a symbolic link inside a
temporary folder, reads it back, and deletes it. The result of that test is the
answer. `developerMode` and `elevated` explain the result. They do not decide
it.

On Windows, `developerMode` reads the registry value
`HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock\AllowDevelopmentWithoutDevLicense`.

If `supported` is `false`, the user interface must block Apply and show
`guidance`.

### 2.2 `list_drives`

Every drive on this computer, with how much room each one has. Answers before a
vault exists, because it is what the first screen shows while asking where the
vault should go.

Arguments: none. Returns `DriveInfo[]`.

```ts
type DriveInfo = {
  root: string                   // for example 'C:\\'
  kind: 'fixed' | 'removable' | 'network' | 'optical' | 'ramDisk' | 'unknown'
  freeBytes: number | null       // space this user may use, not raw free space
  totalBytes: number | null
}
```

On a system without drive letters the single root is reported instead, with
the same shape, so the interface does not need two ways of reading this.

**Every drive letter the operating system reports is listed, of every kind.**
The engine does not decide which ones deserve to be offered, it says what is
there and what kind each one is. On a computer with one drive the answer is one
row.

**`freeBytes` and `totalBytes` are null together when the drive cannot be
read**, which happens with an empty card reader or a network drive that has
stopped answering. They are never zero to mean "do not know". Zero of zero
reads as a completely full drive, which is a different and wrong statement.
A drive with null figures has no meter to draw.

`freeBytes` is the space available to the person running the application, which
on a drive with a quota is smaller than the volume's raw free space. The
smaller number is the true one, because it is what they can actually use.

**Which drive the vault should go on.** Usually the one the installs are
already on, even when it looks like the one with the least room. Consolidating
moves files rather than copying them, so on that drive each group is a rename:
no bytes travel, and the space comes back as the run goes. A vault on a
different drive has to receive a real copy of everything before any duplicate
is removed, so it needs room for the whole collection up front, and the first
run is as slow as reading and writing every file.

A drive that can be unplugged, or that lives on another computer, is a poor
home for a vault whatever its size. Every install points into the vault by
link, so the day the drive is not there, every model in every install stops
loading at once.

### 2.3 `get_app_state`

Arguments: none.

Returns:

```ts
type AppState = {
  vaultRoot: string | null
  vaultInitialized: boolean
  installCount: number
  platform: PlatformReport
  settings: Settings
  lastScanId: string | null            // the scan results come from, see 4.8
  lastCancelledScanAt: string | null   // when the latest scan was cancelled
  lastPlanId: string | null
  interruptedApplies: string[]   // apply identifiers that need recovery
  busy: null | { kind: 'scan' | 'apply' | 'revert', id: string }
}
```

Call this command when the application starts. If `interruptedApplies` is not
empty, the user interface must show the recovery screen before anything else.

### 2.4 `select_vault`

Opens a vault folder, or creates one. This command must run before any command
that touches installs, scans, plans, or vault contents.

Arguments:

```ts
{ path: string, createIfMissing: boolean }
```

Returns:

```ts
type VaultInfo = {
  root: string
  createdAt: string
  volume: string          // the drive root: 'C:\\', with the separator
  freeBytes: number | null    // null when the drive could not be read
  totalBytes: number | null
  fileCount: number
  totalStoredBytes: number
  schemaVersion: number
}
```

**`volume` is the drive root and it carries a trailing separator.** Windows
answers `C:\`, not `C:`, and the engine passes that through. Do not compare it
against the first characters of a path. Comparing `C:\` with `C:` makes every
install on the vault's own drive look like it is on a different one, and the
person is told their files will be copied when they will in fact be renamed.
Normalise both sides before comparing, or compare drives by asking the engine.

On a system without drive letters `volume` is an opaque identifier for the
device, not a path. Treat it as a value to compare for equality and never as
something to display or to join onto.

`freeBytes` and `totalBytes` are null together when the drive could not be
read, which is the same rule as `list_drives`. Never zero for unknown.


Errors: `ioError`, `permissionDenied`, `storeError`, `invalidArgument`.

The engine refuses a vault path that sits inside a registered install. That
refusal uses `conflict`, and nothing is created.

A vault is made only in an empty folder, or in a folder not made yet. A folder
that is already a ComfyVault vault opens as one. A folder that holds anything
else is refused with `conflict`, and nothing in it changes: files already in
the chosen folder would be taken for vault files, never deduplicated, and seen
twice by an install whose extra model folders reach into the vault. The answer
is an empty folder, which the folder picker's New folder makes.

Choosing the vault that is already open answers with its `VaultInfo`.

### 2.5 `get_vault_info`

Returns the open vault's facts, above all how much room its drive has left.
That number is the most-read one in the application, so reading it does not
mean opening the vault again.

Arguments: none. Returns `VaultInfo`, the same shape `select_vault` returns.

Errors: `notInitialized` when no vault is open.

### 2.6 `get_settings` and `update_settings`

```ts
type Settings = {
  metadataLookupsEnabled: boolean   // default true
  hashCacheEnabled: boolean         // default true
  scanExtensions: string[]          // default list is in section 4.2
  minFileSizeBytes: number          // default 1048576
  followExtraModelPaths: boolean    // default true
  scanOutputModelDirs: boolean      // default true
  huggingFaceCacheDirs: string[] | null   // default null
  verifyBeforeDelete: boolean             // default true
}
```

`verifyBeforeDelete` reads a duplicate's bytes again, immediately before
deleting it, and compares them against the copy being kept.

Leave it on. Deleting is the one thing this app does that cannot be undone, and
without it the proof that two files are identical is a hash from an earlier
scan, which may itself have come from a cache row rather than from the file. A
drive with coarse timestamps, which external model drives often have, can hide
a difference from the size and the time alone.

Turning it off makes a consolidation faster and makes the delete a matter of
trust rather than proof.

`huggingFaceCacheDirs` says where the Hugging Face libraries keep their
downloaded models. `null` means work it out from the environment, which is what
a person wants by default. A list covers a cache moved to another drive. An
empty list means do not look at all.

It is a setting rather than something the engine reads from the environment
while it scans, because a scan's result must not depend on machine state
nobody can see.

`update_settings` takes a partial object. Every field is optional. The engine
applies only the fields that are present.

There is no Civitai key and no Hugging Face token in `Settings`. Both live in
Windows Credential Manager, and section 16.1 has the commands.

Looking a model up by hash needs no credential. That was checked against the
live service: the same request unauthenticated, with a bogus bearer token, and
with a token on the query string all return the same answer, byte for byte.
The lookup therefore never sends a token.

A token is kept out of the vault on purpose. The vault is a folder this
product tells the person to carry on a portable drive they might lend, sell,
or back up somewhere shared, and its database would hold the token in the
clear. Credential Manager binds it to the machine and the Windows account.

---

## 3. Installs

### 3.1 `validate_install_path`

Checks a folder before the person registers it. This command never changes
anything on disk.

Arguments:

```ts
{ path: string }
```

Returns:

```ts
type InstallCandidate = {
  valid: boolean
  root: string | null            // the real ComfyUI root, which can be nested
  nestedDepth: number            // 0 when the given folder is itself the root
  markersFound: string[]
  markersMissing: string[]
  contentCheckPassed: boolean
  otherCandidates: string[]      // other roots found under the given folder
  version: string | null
  versionSource: 'comfyui_version.py' | 'pyproject.toml' | null
  modelsDir: string | null
  modelsDirExists: boolean
  extraPathsFile: string | null
  extraPaths: ExtraPath[]
  extraPathsProblems: string[]   // one per complaint, empty when the file is clean
  outputModelDirs: OutputModelDir[]
  reason: string | null          // why valid is false
}
```

How the engine identifies a ComfyUI root. It requires five of these seven
entries in one folder:

```
main.py  nodes.py  folder_paths.py  execution.py  server.py  comfy/  comfy_extras/
```

It then opens `folder_paths.py` and requires the text `folder_names_and_paths`
to appear. That content check removes false positives.

If the given folder fails the test, the engine searches down to three levels
for a folder that passes. It returns the shallowest match in `root`. It returns
every other match in `otherCandidates`. This handles a launcher layout, for
example `C:\Something\ComfyUI-Easy-Install\ComfyUI\`.

`version` comes from `comfyui_version.py`, then from `pyproject.toml`. ComfyUI
added both files in version 0.3.11. An older install records its version
nowhere on disk, so `version` is `null` and `versionSource` is `null`. A
missing version is normal. A missing version never makes `valid` false.

The interface must show "unknown" for a missing version. It must not show
"not affected" for the thumbnail change in section 12.2, because the engine
cannot check that without a version.

```ts
type ExtraPath = {
  section: string        // the top-level key in the YAML file
  category: string       // the model category, after legacy renaming
  rawCategory: string    // the category as written in the file
  path: string           // resolved absolute path
  isDefault: boolean
  exists: boolean
}
```

Two category names are renamed by ComfyUI itself. `unet` becomes
`diffusion_models`. `clip` becomes `text_encoders`. The engine applies the same
renaming and reports both names.

### 3.2 `register_install`

Arguments:

```ts
{ path: string, label?: string }
```

Returns an `Install`. The engine runs the same validation as
`validate_install_path` and rejects an invalid folder with `notAComfyInstall`.

Without `label`, the engine names the install after the folder in `path`, the
folder the person picked, which for a launcher layout is the launcher's folder
rather than the `ComfyUI` folder inside it. If another install already has
that name, compared without regard to case, the engine tries the next folder
up, and so on. If every folder name is taken, the label is the whole `root`.
Installs registered before this rule keep the label they have.

```ts
type Install = {
  id: string
  label: string
  registeredPath: string
  root: string
  modelsDir: string
  version: string | null
  versionSource: string | null
  extraPaths: ExtraPath[]
  outputModelDirs: string[]
  addedAt: string
  lastScanAt: string | null
  lastScanTotals: InstallScanTotals | null
}
```

Errors: `notAComfyInstall`, `alreadyRegistered`, `ioError`, `notInitialized`.

### 3.3 `list_installs`

Arguments: none. Returns `Install[]`.

### 3.4 `refresh_install`

Re-reads the version and the extra model paths from disk. Returns the updated
`Install`.

Arguments: `{ id: string }`.

### 3.5 `update_install`

Arguments: `{ id: string, label: string }`. Returns the updated `Install`.

### 3.6 `unregister_install`

Removes the install from the list. This command never deletes a file. It never
removes a symbolic link. Links that point into the vault keep working.

Arguments: `{ id: string }`. Returns:

```ts
{ removed: true, linksLeftInPlace: number }
```

The user interface must tell the person that the links stay in place.

### 3.7 `list_install_model_dirs`

Lists the folders that can receive a new link. Use this command to fill the
folder picker in the link dialog.

Arguments: `{ id: string }`.

Returns:

```ts
type ModelDirNode = {
  relPath: string        // relative to the root that owns it, for example 'loras\\style'
  absPath: string
  category: string
  origin: 'modelsDir' | 'extraPath' | 'outputDir'
  fileCount: number
  children: ModelDirNode[]
}
```

Returns `ModelDirNode[]`, one per root.

---

## 4. Scan

### 4.1 `start_scan`

Starts a scan. Returns immediately. Progress arrives as events.

Arguments:

```ts
{ installIds?: string[] }    // omit the field to scan every install
```

Returns: `{ scanId: string }`.

Errors: `vaultBusy`, `notInitialized`, `notFound`.

### 4.2 What a scan reads

The engine walks these roots for every install in the scan:

1. `<root>/models` and every folder under it.
2. Every folder listed in `extra_model_paths.yaml`, when
   `followExtraModelPaths` is true.
3. `<root>/output/checkpoints`, `<root>/output/clip`, `<root>/output/vae`,
   `<root>/output/diffusion_models`, and `<root>/output/loras`, when
   `scanOutputModelDirs` is true. ComfyUI registers those five folders as model
   search paths at startup.
4. `<root>/custom_nodes` and every folder under it. These files are counted.
   They are never moved.
5. Every folder in `huggingFaceCacheDirs`. These files are counted. They are
   never moved. When that setting is `null`, the engine works the folders out
   from `HUGGINGFACE_HUB_CACHE`, then `HF_HOME`, then
   `<home>/.cache/huggingface/hub`.

A file enters the scan when both conditions are true:

- The extension matches the list in `scanExtensions`.
- The size is larger than `minFileSizeBytes`.

The default extension list is:

```
.safetensors  .ckpt  .pt  .pth  .bin  .gguf  .onnx  .pt2  .sft  .pkl
```

The first seven come from the product requirement. The last three come from
ComfyUI itself, which treats `.pt2`, `.sft`, and `.pkl` as model weights. The
list is a setting, so the person can change it.

The engine follows symbolic links while it walks. ComfyUI does the same, so the
two agree about which files exist.

### 4.3 Hashing and the cache

The engine computes a SHA-256 hash of the whole file. It uses one hash for two
jobs: it identifies duplicates, and it looks metadata up on Civitai.

The engine caches each hash against the absolute path, the size in bytes, and
the modification time in nanoseconds. If all three match a cached row, the
engine reuses the hash and reads no bytes. If any of the three differs, the
engine reads the file again.

Set `hashCacheEnabled` to `false` to force a full read.

### 4.4 Scan events

| Event | Payload |
|---|---|
| `scan:progress` | `ScanProgress` |
| `scan:done` | `ScanRecord` |
| `scan:error` | `VaultError` |

```ts
type ScanProgress = {
  scanId: string
  phase: 'enumerating' | 'hashing' | 'finalizing'
  installId: string | null
  installLabel: string | null
  filesSeen: number
  filesToHash: number
  filesHashed: number
  bytesToHash: number
  bytesHashed: number
  bytesFromCache: number
  currentPath: string | null
  elapsedMs: number
  etaMs: number | null
}
```

The engine emits `scan:progress` at most four times per second. During
`enumerating`, `filesToHash` and `bytesToHash` grow. During `hashing`, both
stay fixed.

### 4.5 `ScanRecord`

```ts
type ScanRecord = {
  scanId: string
  startedAt: string
  finishedAt: string
  installIds: string[]
  cancelled: boolean
  totals: ScanTotals
  perInstall: InstallScanTotals[]
  errors: ScanError[]
}

type ScanTotals = {
  filesSeen: number
  movableFiles: number
  movableBytes: number
  uniqueContents: number
  uniqueBytes: number
  reclaimableBytes: number      // movableBytes minus uniqueBytes
  duplicateFiles: number
  alreadyLinkedFiles: number
  alreadyLinkedBytes: number
  customNodeFiles: number
  customNodeBytes: number
  hfCacheFiles: number
  hfCacheBytes: number
  skippedFiles: number
  errorCount: number
  bytesRead: number
  bytesFromCache: number
  durationMs: number
}

type InstallScanTotals = ScanTotals & { installId: string, installLabel: string }

type ScanError = {
  path: string
  installId: string | null
  code: ErrorCode
  detail: string
}
```

`reclaimableBytes` is the headline number. It is the space that Apply returns
if the person applies every group.

A scan error never stops the scan. The engine records the path and continues.

### 4.6 `get_scan_entries`

Returns the individual files a scan found. The result is paged, because a scan
can find tens of thousands of files.

Arguments:

```ts
{
  scanId: string
  offset: number
  limit: number                  // maximum 1000
  filter?: {
    installId?: string
    classification?: Classification
    category?: string
    minSizeBytes?: number
    nameContains?: string
    duplicatesOnly?: boolean
  }
}
```

Returns:

```ts
{
  total: number
  offset: number
  entries: ScanEntryWithCount[]
}

type ScanEntryRecord = {
  absPath: string
  relPath: string
  installId: string
  category: string
  sizeBytes: number
  sha256: string | null
  mtimeNanos: string             // nanoseconds since 1970, as text. See below.
  classification: Classification
  linkTarget: string | null      // set when the entry is already a link
}

// What a page of scan entries holds. `occurrenceCount` is counted over the
// whole scan, not over the rows the current filter keeps.
type ScanEntryWithCount = ScanEntryRecord & {
  occurrenceCount: number        // how many paths in this scan hold these bytes
}

type Classification =
  | 'movable'
  | 'customNodes'
  | 'huggingFaceCache'
  | 'alreadyInVault'
  | 'externalLink'
  | 'unreadable'
```

`mtimeNanos` is text, not a number. It is the file's modification time in
nanoseconds. A number that large loses its last digits when JavaScript reads
it: the engine sends 1758240123456789012 and `JSON.parse` returns
1758240123456789000. Divide by 1000000 for milliseconds if a date is what is
wanted. The same applies to `mtimeNanos` on `PlanSource` and `PlanLink`.

### 4.7 `cancel_scan`

Arguments: `{ scanId: string }`. Returns `{ cancelled: true }`.

A cancelled scan emits `scan:done` with `cancelled` set to `true`. A cancelled
scan changes nothing on disk. Hashes already computed stay in the cache.

### 4.8 `get_last_scan`

Arguments: none. Returns `ScanRecord | null`.

It returns the last scan that finished. Totals, plans, orphans and the running
check all come from that scan. A scan that was cancelled later does not replace
it, because a cancelled scan read only part of the installs, and its totals
would say that a scanned computer holds nothing.

When no scan finished yet, it returns the latest cancelled scan, with
`cancelled: true`. It returns `null` only when no scan ran.

`AppState.lastScanId` is the identifier of the same scan.
`AppState.lastCancelledScanAt` is the time that the latest scan was cancelled.
It is `null` when the latest scan finished, and when no scan ran. So after a
full scan and then a cancelled one, `get_last_scan` returns the full scan, and
`lastCancelledScanAt` tells that a later one was cancelled.

A cancelled scan also leaves each install's `lastScanAt` and `lastScanTotals`
as the last finished scan set them.

A vault written by an older build reads the same way. The engine finds its last
finished scan among the stored records.

---

## 5. Plan

### 5.1 `build_plan`

Builds a dry-run plan from a scan. This command never changes anything on disk.

Arguments:

```ts
{ scanId: string }
```

Returns a `ConsolidationPlan`.

Errors: `notFound`, `notInitialized`, `symlinkUnsupported`.

`build_plan` still succeeds when symbolic links are not supported, and it still
builds the groups. The person reads what they would gain, and that is what
sends them to turn Developer Mode on. A plan with no groups would tell them
nothing, in exactly the state where it matters most.

In that case `symlinksSupported` is `false` and `blocked` carries **one** row
with the reason `symlinkUnsupported`, whose path is the vault root. One row,
not one per group: the reason is a fact about the computer, not about any file.
`start_apply` refuses separately, so nothing can act on a plan that only looks
applicable.

### 5.2 `ConsolidationPlan`

```ts
type ConsolidationPlan = {
  planId: string
  scanId: string
  createdAt: string
  vaultRoot: string
  symlinksSupported: boolean   // false: the groups are real, Apply is refused
  groups: PlanGroup[]
  blocked: BlockedRow[]
  totals: PlanTotals
}

type PlanGroup = {
  groupId: string
  sha256: string
  sizeBytes: number
  category: string
  vaultRelPath: string           // for example 'loras\\lora1.safetensors'
  vaultNameAdjusted: boolean
  clashesWith: string | null     // the SHA-256 that already owns the plain name
  vaultAliases: string[]         // other names these copies use, kept as aliases
  source: PlanSource             // which copy becomes the vault file
  links: PlanLink[]              // every place that gets a link. Always `occurrences` long.
  occurrences: number
  distinctFiles: number          // how many real files those paths are
  bytesFreed: number
  singleCopy: boolean
  crossVolume: boolean
  alreadyInVault: boolean        // the vault holds this content already
}

type PlanSource = {
  installId: string
  installLabel: string
  absPath: string
  relPath: string
  sameVolumeAsVault: boolean
  chosenBecause: 'sameVolume' | 'onlyCopy' | 'firstByPath'
  sizeBytes: number              // as measured during the scan
  mtimeNanos: string             // nanoseconds since 1970, as text
}

type PlanLink = {
  installId: string
  installLabel: string
  absPath: string
  relPath: string
  linkName: string               // the name the link keeps
  nameDiffersFromVault: boolean
  isSource: boolean              // this copy's bytes become the vault file
  sharesBytesWithAnother: boolean  // a second name for a file already counted
  sizeBytes: number              // as measured during the scan
  mtimeNanos: string             // nanoseconds since 1970, as text
}

type PlanTotals = {
  groups: number
  groupsFreeingSpace: number
  singleCopyGroups: number
  nameClashes: number
  crossVolumeGroups: number
  bytesFreed: number
  bytesMoved: number
  filesMoved: number
  linksCreated: number
  blockedRows: number
  blockedBytes: number
  vaultFreeBytesIfApplied: number | null   // a prediction, see below
}
```

### 5.3 How the plan decides

**Content the vault already holds.** A copy of a model an earlier run put in
the vault is not moved in again. Its group has `alreadyInVault: true`:

- nothing moves into the vault, and `vaultRelPath` is the existing vault file;
- every copy in `links`, the one marked `isSource` included, becomes a link to
  that file, and its bytes are removed;
- `bytesFreed` counts every distinct copy, `singleCopy` and `crossVolume` are
  false, and the totals' `filesMoved` and `bytesMoved` leave the group out.

Before any copy is touched, the apply reads the vault file and proves it is
this content. A record whose vault file is missing, or is not a real file of
the right size, is not treated this way: its copies move in as for new content.
Undoing such a run puts every copy back and leaves the vault file where it is,
since the earlier run owns it.

**One group per unique content.** Every file with the same SHA-256 belongs to
one group, whatever its name and whatever folder it sits in. The common case:
the same weight file under `loras\awesomeloras\` in one
install and under `loras\newloras\` in another becomes one group.

**The vault path.** The engine takes the model category, then the file name.
`loras\awesomeloras\lora1.safetensors` becomes `loras\lora1.safetensors`. The
sub-folder disappears inside the vault. The sub-folder stays in the install,
because the link stays where the file was.

**Name clashes.** Two different contents can carry the same file name. The
first group keeps the plain name. The second group takes
`<stem>__<first 8 of SHA-256><extension>`, for example
`lora1__3F9A2C17.safetensors`. Both survive. `vaultNameAdjusted` is `true` on
the second group.

**Every copy gets a link, including the one that moves.** `links` holds one
entry per place that held the file, so `links.length` always equals
`occurrences`. The copy whose bytes become the vault file is in there too, with
`isSource` set to `true`: its old place gets a link like every other, and
nothing is deleted there because the file moved. The same copy is also named in
`source`, which is where the reason it was chosen lives.

Read `links.length` as "this many places get a link". Do not subtract one.

**The link keeps its own name.** A link is always created with the name the
file had in that install. The vault file name can differ. When it differs,
`nameDiffersFromVault` is `true`.

**Which copy becomes the vault file.** The engine prefers a copy that already
sits on the vault volume, because that move is a rename and takes no time and
no extra space. If no copy sits on the vault volume, the engine takes the
first copy by sorted path. `chosenBecause` reports the rule that fired.

A group with one copy always reports `onlyCopy`, even when that copy happens
to sit on the vault volume. No rule had to fire, because there was no choice
to make, and "the only copy" is the truer sentence to put in front of a
person than "it was already on the right drive".

**Two names for one file free nothing.** Some of a group's paths can be hard
links to each other: two names, one set of bytes. Removing one returns no space
while the other name remains. `distinctFiles` counts the real files, which is
lower than `occurrences` when that happens, and `bytesFreed` is
`sizeBytes * (distinctFiles - 1)`. Each such path carries
`sharesBytesWithAnother`, so a row can say it rather than the total quietly
disagreeing with the rows.

When the system cannot say which file a path names, the engine counts it as its
own file. That reports less space than there may be, which is the right
direction for a number the person checks against their drive afterwards.

**Single copies.** A file that exists once still moves into the vault. It
frees nothing. `singleCopy` is `true` and `bytesFreed` is `0`. The user
interface must present these separately, because the person expects the space
number to match the row count.

**Blocked rows.** A row that cannot move never appears in a group. It appears
in `blocked` with a reason.

**A plan is derived fresh, every time.** `build_plan` re-checks the machine: it
asks again whether each file still matches what the scan read, and whether
another program holds it open. So the plan changes as the machine changes, and
it changes in the direction people expect.

When the person closes ComfyUI and the interface builds the plan again from the
same scan, the plan gets **bigger**:

- A file that was held open rejoins its group, so a content that looked like a
  single copy becomes a duplicate and starts saving space.
- The copy that gets kept can change. A copy on the vault's own drive is worth
  keeping, because moving it is a rename, and if that copy was the one held
  open then closing ComfyUI changes which file moves.

Do not clear a flag on the old plan. Build it again.

**`vaultFreeBytesIfApplied` is a prediction, not a reading.** It is what the
vault's drive would have free if this plan ran. The record of a finished run
carries `vaultFreeBytesBefore` and `vaultFreeBytesAfter`, and those two are
read off the drive. Do not show a prediction where the screen promises a
measurement, and do not subtract one from the other.

It was called `vaultFreeBytesAfter` until a finished screen showed a computed
figure where a person expected a measured one. The names are different now so
that cannot happen by reading the wrong one.

Null when the drive could not be read. When it is null, the engine also does
not block anything for lack of space, because it has not measured any.

```ts
type BlockedRow = {
  absPath: string
  installId: string | null
  installLabel: string | null
  sizeBytes: number
  sha256: string | null
  reason: BlockReason
  detail: string
}

type BlockReason =
  | 'fileLocked'            // another program holds it open
  | 'fileChanged'           // size or modification time changed after the scan
  | 'fileMissing'           // it disappeared after the scan
  | 'permissionDenied'
  | 'inCustomNodes'         // counted, never moved
  | 'inHuggingFaceCache'    // counted, never moved
  | 'alreadyInVault'        // it is already a link into this vault
  | 'externalLink'          // it is a link that points somewhere else
  | 'symlinkUnsupported'
  | 'vaultInsideInstall'
  | 'targetExistsNotLink'   // something already sits at the vault path
  | 'notEnoughSpace'
  | 'unsafeVaultPath'      // the folder name would put it outside the vault
  | 'readError'
```

### 5.4 `get_plan`

Arguments: `{ planId: string }`. Returns `ConsolidationPlan`.

---

## 6. Apply

### 6.1 `start_apply`

Applies the groups the person selected. Returns immediately. Progress arrives
as events.

Arguments:

```ts
{
  planId: string
  groupIds: string[]                      // only these groups are applied
  verify: 'sizeAndMtime' | 'rehash'       // default 'sizeAndMtime'
  stopOnError: boolean                    // default false
}
```

Returns `{ applyId: string }`.

Errors: `vaultBusy`, `notFound`, `symlinkUnsupported`, `notInitialized`.

`groupIds` must be explicit. The engine never applies a group the caller did
not name. To apply everything, send every group identifier.

The engine stores the list. `resume_apply` finishes only those groups, never
the rest of the plan. A run recorded by an older build has no stored list, and
then a resume does nothing.

### 6.2 What Apply guarantees

**Apply does only what the plan says.** The engine reads the stored plan. It
does not re-plan and it does not widen the work.

**Apply checks every file before it touches it.** The check compares the size
and the modification time against the values the scan recorded. If either
differs, the engine stops that row, records `fileChanged`, and moves to the
next group. It never moves a file it has not checked.

Set `verify` to `rehash` to read every file again and compare the SHA-256. That
is slower and it is the strongest check. Use it when a long time passed between
the scan and the apply.

**Apply never deletes bytes before the replacement is in place.** For each
duplicate copy, the engine renames the file aside first, creates the link, and
deletes the renamed file last. If any step fails, the engine renames the file
back.

**Apply journals every step before it performs it.** The journal lives in the
vault database. A crash leaves a readable journal, so the work can be finished
or reverted.

**Apply is per-group atomic.** A group either completes or leaves the disk as
it was. A failure inside one group never leaves another group half done.

**Cross-volume moves are copy, verify, then delete.** The engine copies to a
temporary file inside the vault, flushes it to disk, compares the SHA-256, and
only then puts it in place and removes the source.

### 6.3 Apply events

| Event | Payload |
|---|---|
| `apply:progress` | `ApplyProgress` |
| `apply:done` | `ApplyRecord` |
| `apply:error` | `VaultError` |

`bytesToMove` and `bytesMoved` count only the groups whose move is a real copy
to another drive. A move within one drive is a rename, which moves no bytes, so
counting it would make the time remaining pessimistic at the start and then
jump. Use `groupIndex` and `groupTotal` for a progress bar that covers the
whole run.

**On a run where the vault is on the installs' own drive, both are zero for the
whole run, and that is correct.** Nothing is copied. Do not draw a byte counter
from them there; it reads "0 MB of 0 MB" from start to finish, which looks like
a broken screen rather than a true statement about renames.

`bytesFreed` is the byte figure that moves on every run. It rises as each
duplicate goes, whichever drive the vault is on. If the screen wants bytes
while the work runs, that is the one.

```ts
type ApplyProgress = {
  applyId: string
  phase: 'preflight' | 'applying' | 'finalizing'
  groupIndex: number
  groupTotal: number
  currentGroupId: string | null
  currentPath: string | null
  step: 'verifying' | 'moving' | 'linking' | 'cleaning'
  bytesMoved: number
  bytesToMove: number    // only the bytes really copied. See below.
  bytesFreed: number
  filesMoved: number
  linksCreated: number
  failures: number
  elapsedMs: number
  etaMs: number | null
}
```

### 6.4 `ApplyRecord`

```ts
type ApplyRecord = {
  applyId: string
  planId: string
  state: 'running' | 'completed' | 'completedWithErrors' | 'cancelled'
       | 'interrupted' | 'partlyReverted' | 'reverted' | 'setAside'
  startedAt: string
  finishedAt: string | null
  groupsRequested: number
  groupIds: string[]             // the groups this run was asked to do
  groupsApplied: number
  groupsFailed: number
  bytesFreed: number
  filesMoved: number
  linksCreated: number
  vaultFreeBytesBefore: number | null   // read off the drive, before the run
  vaultFreeBytesAfter: number | null    // read off the drive, after the run
  failures: ApplyFailure[]
  revertible: boolean
  lastUndoStepAt: string | null         // when an undo last began a step
}

type ApplyFailure = {
  groupId: string
  absPath: string
  reason: BlockReason
  detail: string
}
```

### 6.5 `cancel_apply`

Arguments: `{ applyId: string }`. Returns `{ cancelled: true }`.

The engine stops as soon as it can. A group that was part way through is undone
first, so a group is still all or nothing. Everything already applied stays
applied, and stays revertible.

A group stopped by a cancel is **not** reported as a failure. The person
stopped it, so it is left out of `failures`, and `state` is `cancelled`.

This applies only to the group the cancel interrupted. A group that broke on
its own earlier in the same run stays in `failures`, and `state` is still
`cancelled`. Show both. A file the engine could not move is never reported
anywhere else, so a result screen that hides it because the run was stopped
leaves the person believing a file was moved when it was not.

### 6.6 `get_apply_result` and `list_applies`

**`vaultFreeBytesBefore` and `vaultFreeBytesAfter` are measurements.** The
engine reads the vault's drive once before the first file moves and once after
the last one. Show them as they are. Do not compute either from the other.

`bytesFreed` is a different kind of number: it is what the run accounted for.
The two can honestly disagree. Something else on the computer may write or
delete files while the run is going, a file may be sparse and never have
occupied what its size claimed, and a drive rounds to its allocation unit. A
screen that derives "before" by subtracting `bytesFreed` from the reading taken
afterwards is not reporting a measurement, and it will agree with itself on any
tree, including one where nothing happened.

Both are null when the drive could not be read. Null is not zero.

A run that was undone keeps these two, along with `bytesFreed`, `filesMoved`
and `linksCreated`. They record what the run did. `state` is `reverted`, and
that is what says it was put back.

`get_apply_result` takes `{ applyId: string }` and returns `ApplyRecord`.

`list_applies` takes no arguments and returns `ApplyRecord[]`, newest first.

### 6.7 `get_interrupted_applies`

Arguments: none.

Returns:

```ts
type InterruptedApply = {
  applyId: string
  planId: string
  startedAt: string
  stepsDone: number
  stepsPending: number
  description: string        // one sentence for the person
  affectedPaths: string[]
  blocked: boolean           // names places outside the vault and the installs
  blockedPaths: string[]     // those places
}
```

Returns `InterruptedApply[]`. If the list is not empty, the user interface must
resolve it before it allows a new scan or a new apply.

`description` states only what the journal records: how many steps were done
and how many were left. It does not say whether anything was lost, because
that is not known about a run nobody has checked yet.

`blocked` is true when the run names places that are neither in the vault nor
in a folder of a registered install that is a ComfyUI install on this computer
now. `blockedPaths` lists them. Such a run is neither finished nor undone:
`resume_apply` and `revert_apply` reject it with `pathOutsideBoundary`. The
honest causes a person meets are the vault being opened on a different
computer, or an install moved or removed after the run. A vault someone else
prepared can cause it too, so the screen must not present it as always
harmless. The one way out is `set_aside_run`.

#### `set_aside_run`

Arguments: `{ applyId: string }`. Returns the `ApplyRecord`, with `state`
`setAside` and `revertible` false.

It changes only the run's record. Nothing on the disk moves: the links the run
made keep pointing into the vault, and any file the run set aside or had not
reached stays where it is. A model the run was in the middle of may have no
file and no link at its place, so it may not load until the run is finished or
undone. The run stops being listed by `get_interrupted_applies` while its
places cannot be proved. Once they can, for example when an unplugged drive is
back, it is listed again with `blocked: false`, and `resume_apply` and
`revert_apply` accept it. Setting a run aside is not final, but the person
confirms it knowing what it leaves, with `blockedPaths` in front of them.

It rejects with `conflict` for a run whose `state` is not `running`, and for a
cut-off run that is not `blocked`, which can be finished or undone instead.

### 6.8 `resume_apply` and `revert_apply`

`resume_apply` takes `{ applyId: string }` and finishes an interrupted run. It
re-checks every file it has not yet touched. It emits the apply events. It
rejects with `conflict` unless the run's `state` is `running`, which is what a
run cut off part way is left as, or `setAside`. A run that finished, was stopped, or was
undone is not finished again.

#### Every place a run names is proved first

The journal, the stored plan and the stored installs are read from the vault's
database, which travels with the vault and may not have been written on this
computer. Before `resume_apply`, `revert_apply` or `preview_revert` touches or
reports anything, every path the run names is proved to be one of two places:

- inside the vault, and not inside its `.comfyvault` folder;
- inside a folder the scan walks for a registered install whose folder is a
  ComfyUI install on this disk now, and not inside its `custom_nodes`.

A file a run moves, links or puts back must also have a model file's name: a
plain name with one of the model extensions built into the engine, the default
`scanExtensions` list. The vault's own `scanExtensions` does not count here,
because it is stored in the same database the check does not trust.

If any path fails, the whole call rejects with `pathOutsideBoundary`
before a file is touched. `detail` lists every such path, and `path` is the
first. `start_apply` proves each group of the stored plan the same way, and a
group that fails is reported in `failures` and not touched.

A stored install is inspected on the disk again before its folders are scanned
or receive a link. One whose folder is not a ComfyUI install now is left out of
a scan and a plan, and `create_link` and `create_model_folder` reject with
`notAComfyInstall`.

`revert_apply` takes `{ applyId: string }` and undoes a run, in reverse order.
It emits `revert:progress`, `revert:done` and `revert:error`. `revert:done`
carries the `ApplyRecord`. `revert:progress` carries its own payload, described
below.

#### What an undo copies, and why

An undo puts every file back at its original path. The files come back in two
different ways:

- **The kept copy comes back by a rename.** Its bytes are the vault file. On
  one drive the rename is instant and takes no room.
- **Each duplicate comes back as a copy of the vault file.** Apply deleted the
  duplicate's bytes, which is what freed the room. One vault file cannot become
  several separate files by renames, so each duplicate is written again.

A copy therefore costs time, and it takes room on the drive the file goes back
to. Before the undo starts, the engine checks that room on each drive. If a
drive is short, the undo stops before it touches a file, with `revert:error`,
code `ioError`, and `detail` giving the bytes needed and the bytes free.

A copy keeps what the file system knew about the file:

- A sparse file stays sparse. Ranges of zeros are left unwritten.
- An NTFS compressed file stays compressed.
- The modification time is the one the duplicate had before the run. A run
  recorded by an older build did not record it, and then the copy takes the
  vault file's time.

A file that was not sparse is written in full, zeros included.

A run whose vault is on another drive copies the kept copy back as well. The
copy is staged beside the place it goes back to, checked, and then renamed into
place.

#### An undo that does not finish

`cancel_apply` with the same `applyId` stops an undo, and `revert:error`
arrives with code `cancelled`. The files already put back stay back. The rest
stay in the vault behind their links.

**Every path loads in ComfyUI at every moment of an undo.** A file comes back
by one rename onto the link that stood in its place. A copy is written to a
temporary file beside that link, checked, and then renamed over it. So a path
holds either the link or the file, whether the undo finishes, is stopped,
fails, or the power goes.

`state` is `partlyReverted` from the moment an undo starts until it finishes,
when it becomes `reverted`. A run still `partlyReverted` when no undo is
running was stopped, failed, or was cut off when the app closed. `revertible`
stays true, and `revert_apply` again puts back the rest. `finishedAt` still
says when the run itself finished.

`preview_revert` on such a run gives `filesAlreadyBack`, and the cost of the
rest.

`lastUndoStepAt` is when an undo last began a step on the disk, in the same
form as `finishedAt`. It is null on a run no undo has touched. It is written
before each step, so a step cut off by a crash is never missed. It stays null
when an undo is refused or stopped before its first step. A stopped copy can
move it without any file coming back, which errs on the safe side. Treat any
scan that finished before `lastUndoStepAt` as out of date, for a
`partlyReverted` run and a `reverted` one alike. A record from an older build
has no such field. Read it as null.

#### `preview_revert`

`preview_revert` takes `{ applyId: string }` and returns what an undo will
cost, without doing it. Call it before the person confirms an undo.

```ts
type RevertPreview = {
  applyId: string
  filesAlreadyBack: number     // put back by an earlier undo of this run
  filesRenamedBack: number     // instant, and take no room
  filesCopiedBack: number      // take time and room
  bytesToCopy: number          // the size of the files the copies write
  drives: RevertDrive[]
}

type RevertDrive = {
  volume: string               // same form as VaultInfo.volume, for example 'C:\\'
  predictedRoomBytes: number   // what the copies are expected to occupy there
  freeBytes: number | null     // read off the drive. null when it did not answer
}
```

`bytesToCopy` sets the time. `predictedRoomBytes` sets the room. The two are
different numbers for a sparse or compressed file: a sparse model of 20 GB can
occupy a few kilobytes, and its copy occupies the same. Show the room from
`predictedRoomBytes`, never from `bytesToCopy` or `bytesFreed`.

`predictedRoomBytes` is a prediction. It is what the vault file occupies, once
for each copy. `freeBytes` is a measurement. Null is not zero.

`drives` is empty when nothing is copied.

`preview_revert` rejects exactly where an undo would refuse before it starts:
`notFound` for an unknown run, and `conflict` for a run already undone or a run
that something later depends on. It does not check room. It reports it.

#### `revert:progress`

```ts
type RevertProgress = {
  applyId: string
  phase: 'restoring' | 'finalizing'
  stepIndex: number            // journal steps undone so far
  stepTotal: number
  currentPath: string | null   // the path being put back
  action: 'removingLink' | 'renamingBack' | 'copyingBack' | 'tidying'
  filesPutBack: number         // files at their original path again
  filesToPutBack: number
  linksRemoved: number         // links removed from installs
  bytesCopied: number
  bytesToCopy: number          // 0 when nothing is copied
  elapsedMs: number
  etaMs: number | null
}
```

Updates arrive at most four times a second, and during a copy as well as
between steps. `bytesCopied` rises while a file is copied. Use it for a bar
when `bytesToCopy` is above zero, and `stepIndex` of `stepTotal` otherwise.

`linksRemoved` counts the links in installs, as `linksCreated` did for the
apply. The extra names a vault file carries are also removed, and not counted.

The last update has `phase: 'finalizing'`, and then `revert:done` arrives.

`revert_apply` and `preview_revert` reject with `conflict` when a different file
now sits where a model was consolidated from, for example because a downloader
replaced the link with a new version. The vault file is then the only copy of
the model, and it is never deleted to make way. `detail` names both places.
Moving the other file away and undoing again finishes the undo. Where the same
bytes sit there, the undo goes ahead, and the vault file is the one dropped.

`revert_apply` rejects with `conflict` when anything done later still uses the
files this run created. Renaming a model in the vault with
`set_canonical_name` is the common case. `detail` lists the paths in the way.
Undo the later change first.

A resumed run is undone as one run. Resuming continues the same journal rather
than starting a new one, so the whole of it comes back.

---

## 7. Links

### 7.1 `create_link`

Creates one symbolic link inside an install, pointing at a vault file.

Arguments:

```ts
{
  installId: string
  sha256: string
  relativeDir: string        // relative to the install root, for example 'models\\loras\\style'
  linkName?: string          // defaults to the vault file name
  createDir: boolean         // default false
}
```

Returns a `LinkRecord`.

```ts
type LinkRecord = {
  id: string
  installId: string
  absPath: string
  relPath: string
  linkName: string
  sha256: string
  vaultRelPath: string
  createdAt: string
  createdBy: 'apply' | 'manual' | 'download'
  applyId: string | null         // the run that made it, null when made by hand
}

type LinkState = 'ok' | 'dangling' | 'replaced' | 'missing'

// What `list_links` returns. The stored record, with what the path looks like
// on disk at the moment of the call. `state` is measured, never stored, so it
// is on this shape and not on `LinkRecord`.
type LinkWithState = LinkRecord & { state: LinkState }
```

Errors: `pathOutsideBoundary`, `symlinkUnsupported`, `conflict`, `ioError`,
`notFound`.

`relativeDir` must resolve inside the install's models folder, inside one of
its extra model paths, or inside one of its output model folders. Any other
path is rejected with `pathOutsideBoundary`. The engine resolves the path fully
before it checks, so `..` cannot escape.

If `createDir` is `false` and the folder does not exist, the engine rejects with
`notFound`. If `createDir` is `true`, the engine creates the folder.

If something already sits at the target name, the engine rejects with
`conflict`. It never overwrites.

### 7.2 `remove_link`

Arguments: `{ linkId: string }`. Returns `{ removed: true }`.

The engine removes the symbolic link only. It never removes the vault file.
It rejects with `conflict` if the path is not a symbolic link, because that
means a real file took its place.

### 7.3 `create_model_folder`

Arguments:

```ts
{ installId: string, relativeDir: string }
```

Returns `{ absPath: string, created: boolean }`.

The same boundary rule as `create_link` applies.

### 7.4 `list_links`

Arguments:

```ts
{ installId?: string, sha256?: string, state?: LinkState }
```

Returns `LinkWithState[]`.

---

## 8. Vault contents and cleanup

### 8.1 `list_vault_files`

Arguments:

```ts
{
  offset: number
  limit: number             // maximum 1000
  filter?: {
    category?: string
    nameContains?: string
    minSizeBytes?: number
    orphansOnly?: boolean
    withAliasesOnly?: boolean
  }
  sort?: 'name' | 'size' | 'addedAt' | 'linkCount'
  descending?: boolean
}
```

Returns:

```ts
{
  total: number
  offset: number
  files: VaultFile[]
}

type VaultFile = {
  sha256: string
  canonicalName: string
  category: string
  vaultRelPath: string
  sizeBytes: number
  addedAt: string
  aliases: string[]            // other names this content carries inside the vault
  linkCount: number            // live links from installs
  links: LinkRecord[]
  metadata: ModelMetadata | null
  present: boolean             // the file exists on disk
}
```

### 8.2 How names work inside the vault

The vault stores one real file per unique content. When the same content
arrived under a second name, the vault also stores that second name as a
symbolic link beside the real file. The vault therefore shows every name the
content was ever known by.

`canonicalName` is the real file. `aliases` are the in-vault links.

### 8.3 `list_contents`

One row per unique content, across the vault **and** the installs. This is the
Library's own listing: a model is counted once, whether its bytes are already
in the vault or still sitting in four installs.

Without this, the Library has to be stitched from `build_plan` groups plus
`list_vault_files`, which means paging two lists to draw one screen.

Arguments are the same shape as `list_vault_files` in section 8.1, with one
extra filter.

```ts
{
  offset: number
  limit: number                  // maximum 1000
  filter?: VaultFilter & {
    inVault?: boolean            // true: already in the vault. false: still out
                                 // in the installs. Absent: both.
  }
  sort?: 'name' | 'size' | 'addedAt' | 'linkCount' | 'occurrences'
  descending?: boolean
}
```

Returns:

```ts
type ContentPage = {
  total: number
  offset: number
  rows: ContentRow[]
  scanId: string | null    // where the out-of-vault rows came from. null means
                           // nothing has been scanned, so only the vault shows.
}

type ContentRow = {
  sha256: string
  name: string             // the vault's name for it, or the name on disk
  category: string
  sizeBytes: number
  aliases: string[]
  occurrenceCount: number  // places on disk that hold this content right now
  linkCount: number        // how many of those places are links into the vault
  inVault: boolean
  installIds: string[]
  addedAt: string | null   // when the vault took it. null while it is still out.
  metadata: ModelMetadata | null
}
```

A content that is half consolidated, with some copies linked and one still a
real file, is **one** row. `occurrenceCount` counts every place and
`linkCount` counts the linked ones, so `occurrenceCount - linkCount` is how
many real copies are left.

After a consolidation, `occurrenceCount` equals `linkCount` for every content
that was consolidated: the places that held it now hold links to it, and each
place is counted once. It is not the number of places plus the number of links.
A row where the count is exactly twice the links is reporting the same paths
twice.

`orphansOnly` here means a content the vault holds that nothing on disk reaches
any more. A model still sitting in an install is not an orphan: it is there.

### 8.4 `list_name_groups`

Lists contents that carry more than one name.

Arguments: none.

Returns:

```ts
type NameGroup = {
  sha256: string
  sizeBytes: number
  category: string
  canonicalName: string
  names: Array<{
    name: string
    isCanonical: boolean
    vaultRelPath: string
    usedByLinks: number        // install links that resolve through this name
    seenInInstalls: string[]
  }>
}
```

Returns `NameGroup[]`.

### 8.5 `set_canonical_name`

The person picks the name the vault keeps as the real file.

Arguments: `{ sha256: string, name: string }`.

Returns the updated `VaultFile`.

The engine makes the chosen name the real file and turns the previous real name
into an in-vault link. It then repoints every install link to the new real
path, so no link resolves through a second link. Every step is journaled and
revertible.

### 8.6 `remove_alias`

Removes one in-vault name. This is a separate action, on purpose. Names stay by
default, so saved workflows keep working.

Arguments: `{ sha256: string, name: string }`.

Returns `{ removed: true }`.

The engine rejects with `conflict` if the name is the canonical name. It
rejects with `conflict` if any install link resolves through that name, and
`detail` lists the links.

### 8.7 `list_orphans`

Lists vault files that no install links to.

Arguments: none. Returns `VaultFile[]` where `linkCount` is `0`.

The engine verifies each recorded link on disk before it answers. A link that
disappeared no longer counts.

### 8.8 `delete_vault_file`

Deletes a vault file and every name it carries. **This action cannot be
reverted.** The bytes are gone.

Arguments:

```ts
{
  sha256: string
  confirm: string        // must equal the sha256
  removeLinks?: boolean  // default false
}
```

Returns:

```ts
type Deleted = {
  deleted: true
  bytesFreed: number
  linksRemoved: string[]   // every install link removed, as an absolute path
}
```

The user interface must ask the person to confirm, and must say that the action
cannot be undone.

#### Without `removeLinks`

The engine rejects with `conflict` when any install link points at the file.
Remove the links first. `linksRemoved` is empty.

#### With `removeLinks: true`

The engine also removes every link to the file, in every install, and in
installs that are no longer registered. The model disappears from every
ComfyUI that used it. The user interface must say so before the person
confirms.

The engine checks everything it will remove before it removes anything. It
refuses, and removes nothing, when:

| Code | When |
|---|---|
| `vaultBusy` | A scan, a consolidation, or an undo runs. |
| `conflict` | A recorded link path now holds a real file, or a link that leads somewhere other than this file. `detail` lists every such path, and `path` is the first one. |
| `conflict` | A second name of the file inside the vault is no longer a link to it. Same `detail` and `path`. |
| `conflict` | The vault file is not the file the vault recorded: it is not a regular file, or its size differs. |
| `fileLocked` | Another program holds the vault file open. `path` names the file. |

It also refuses with `invalidArgument` when `confirm` does not match, and with
`notFound` when the vault does not hold the model.

Then it removes, in this order:

1. Every install link.
2. Every second name inside the vault.
3. The vault file.
4. The records of every link to the file, and the vault file's own record.

Each removal is written to the journal before it happens. The journal belongs
to no run, so it never appears as a run to finish or undo. It makes an undo of
the run that put the file in the vault refuse with `conflict` before it moves
anything, because that undo needs the file. That refusal says "One of this
run's models was deleted in Cleanup, so this run can no longer be undone.
Nothing was changed.", and `detail` lists the deleted model's paths, separated
by commas. `revert_apply` and `preview_revert` both answer it.

Only a delete that happened after the run started counts. The undo of a run
made after the delete, for example after the same model came back and was
consolidated again, is not refused because of it.

If the delete stopped part way (see below), the model is still in the vault,
and the undo refuses with `conflict` and a different message: "A delete of one
of this run's models stopped part way, so this run was not undone. Nothing was
changed. Finish the delete in Cleanup. After that, this run can no longer be
undone." `detail` lists the paths the same way.

**All or nothing.** If the disk refuses a removal, the engine puts back every
link it already removed, and rejects with the disk's code (`ioError`,
`permissionDenied` or `fileLocked`). The message ends with "Nothing was
deleted, and every link it removed was put back." `path` names the path that
failed. This covers another program opening the file between the check and
the delete.

Two more refusals can come after the removals started, and they put the links
back the same way. Both reject with `conflict`, and `path` and `detail` name
the path:

- A link path stopped being the link that was checked: each link is checked
  again right before it is removed, so a real file that took its place is
  never removed.
- A new link to the model was recorded while it was being deleted. The engine
  makes `create_link`, `remove_link`, `set_canonical_name`, `remove_alias`,
  `delete_vault_file` and `remove_dangling_links` wait while a delete with
  links runs, so this refusal is for anything that did not wait.

If a link cannot be put back, the message says how many were not, and `detail`
lists them. The vault file is still there in that case, and the other installs
still load the model.

A delete stopped by a crash, or by the power going, leaves one of three
states:

- Every link and the file still there, when it stopped before the first
  removal.
- Some links and second names gone, and the others and the file still there.
  The installs whose link is gone no longer load the model.
  `check_vault_health` lists the model in `stoppedDeletes`, and `ok` is false.
- No links and no file, with the records still there.

Run the same delete again to finish it. A link already gone is not counted in
`linksRemoved`, and a file already gone frees `0` bytes.

### 8.9 `check_vault_health`

Arguments: none.

Returns:

```ts
type VaultHealth = {
  checkedLinks: number
  checkedFiles: number
  danglingLinks: LinkRecord[]          // the link exists, the target does not
  replacedLinks: LinkRecord[]          // a real file sits where a link belonged
  missingVaultFiles: VaultFile[] // recorded in the database, absent on disk
  foreignFiles: string[]         // files in the vault folder the database does not know
  stoppedDeletes: VaultFile[]    // a delete with links stopped part way; see 8.8
  ok: boolean
}
```

A model in `stoppedDeletes` is still in the vault, and some installs lost
their link to it. The user interface must offer to delete it again with
`delete_vault_file` and `removeLinks: true`, which finishes the delete. It
leaves the list once the delete finishes. A delete that failed and put its
links back is never listed.

A dangling link is the most serious result. ComfyUI lists a dangling link in
its model menu, and then fails to load it. Worse, a custom node that
re-downloads a missing model writes through the dangling link and puts the file
inside the vault. The user interface must show `danglingLinks` first and must
offer to remove them.

---

## 9. Is a model used

### 9.1 `check_model_usage`

Searches saved workflow files for a file name.

Arguments:

```ts
{ names: string[], installIds?: string[] }
```

Returns:

```ts
type UsageResult = {
  name: string
  used: boolean
  searched: boolean     // were any saved workflow files searched at all?
  matches: UsageMatch[]
  method: string        // one of the two sentences below
}

type UsageMatch = {
  installId: string
  installLabel: string
  workflowPath: string
  workflowName: string
}
```

Returns `UsageResult[]`, one per requested name.

`method` is one of exactly two sentences. When workflow files were searched:

> The file name was searched for as plain text inside saved workflow files.

When there were no saved workflow files to search at all:

> No saved workflow files were found, so nothing was searched. A workflow that
> was never saved lives in the browser, where this app cannot see it.

The second sentence matters. A person whose workflows only ever lived in the
browser would otherwise read "not used" for every model they own and believe
the app had checked.

`searched` carries the same fact as a boolean, so the interface never has to
read it back out of the sentence. When `searched` is `false`, do not present
the model as used or unused at all: nothing was checked. `used: false` with
`searched: false` is not an answer about the model.

**This check is deliberately shallow.** The engine looks for the file name as
text inside the JSON. It does not parse the graph. It does not resolve node
inputs. A match means the name appears. It does not prove the model runs.

The user interface must show the `method` sentence next to the result. A person
must never read "not used" as "safe to delete" without being told what the
check actually did.

The engine searches these locations under each install root:

```
every .json file under user/, up to eight folders deep
any file named workflow.json, up to four folders deep, outside user, models,
  custom_nodes, output, input, temp, .git, venv, .venv and python_embeded
```

Saved workflows and subgraphs live under `user/`, and so do other JSON files,
such as settings. A match under `user/` can therefore come from a file that is
not a workflow.

Files larger than 50 MB are skipped and reported.

Saved workflows only exist on disk when the person pressed Save. A draft that
was never saved lives in the browser, and the engine cannot see it. The user
interface must say so.

---

## 10. Metadata

### 10.1 `get_metadata`

Arguments: `{ sha256: string, refresh?: boolean }`.

Returns `ModelMetadata | null`.

```ts
type ModelMetadata = {
  sha256: string
  source: 'civitai'
  fetchedAt: string
  found: boolean
  modelName: string | null
  modelType: string | null         // 'Checkpoint', 'LORA', 'VAE', and others
  versionName: string | null
  baseModel: string | null         // free text, not a closed list
  triggerWords: string[]
  nsfw: boolean
  nsfwLevel: number
  civitaiModelId: number | null
  civitaiVersionId: number | null
  pageUrl: string | null
  downloadUrl: string | null
  ambiguous: boolean               // the hash matched more than one model version
}
```

The engine keeps no picture of a model. The app shows none, so nothing that
could make the window load an image from the network is collected. The
window's content rules allow no remote image at all, and a test holds both of
them, the one in `tauri.conf.json` and the one `index.html` carries, to that.

A cached answer is checked again every time it is read, whoever wrote it.
`downloadUrl` is held to Civitai over HTTPS, and `pageUrl` to exactly the page
the engine builds, `https://civitai.com/models/<number>` with an optional
`?modelVersionId=<number>`. An address that fails is dropped, and `pageUrl` or
`downloadUrl` is then null. Picture fields in an answer cached by an older
build are ignored when it is read.

### 10.2 `fetch_metadata_batch`

Looks several hashes up in one request. Civitai accepts 100 SHA-256 hashes per
request, so the engine splits a longer list into chunks of 100.

Arguments: `{ sha256: string[], refresh?: boolean }`.

Returns `ModelMetadata[]`.

Civitai returns the matches in an arbitrary order and drops the misses. The
engine maps each result back to its input by the hash inside the response, and
returns a `found: false` record for every hash that did not come back.

One file can match several model versions, because people re-upload identical
files. When that happens, the engine keeps the lowest version identifier, which
is the original upload, and sets `ambiguous` to `true`.

### 10.3 Offline behavior

A metadata lookup is optional and it never breaks anything.

- If `metadataLookupsEnabled` is `false`, the engine answers from the cache
  only.
- If the network fails, the engine answers from the cache, and returns `null`
  for anything not cached. It does not reject.
- A file with no match is normal. `found` is `false`. That is not an error.
- The engine never blocks a scan, a plan, or an apply on a metadata lookup.

`networkUnavailable` is returned only by `fetch_metadata_batch` when the caller
asked for `refresh` and the network refused. Everything else degrades quietly.

### 10.4 `clear_metadata_cache`

Arguments: none. Returns `{ cleared: number }`.

### 10.5 `open_civitai_page`

Opens a model's page on Civitai in the default browser.

Arguments: `{ modelId: number, versionId?: number | null }`. Pass
`civitaiModelId` and `civitaiVersionId` from `ModelMetadata`. Both are whole
numbers. Any other value, a string included, rejects with `invalidArgument`
before anything opens.

Returns `null`. It rejects with `ioError` when Windows does not open the page.
It answers before a vault is chosen.

The engine builds the address, `https://civitai.com/models/<modelId>` with
`?modelVersionId=<versionId>` when there is a version. The window's opener
plugin is not allowed to open any Civitai address. Its scope matches the raw
text of an address, and its wildcard also matches `/`, `@`, `?`, `#`, spaces
and quotes. So it cannot hold "a model page and nothing else", and the window
names the page by its numbers instead.

---

## 11. Running programs and locked files

### 11.1 `get_running_comfy`

Detects ComfyUI processes. Windows refuses to move a file that a program holds
open, so this check runs before an apply.

Arguments: none.

Returns:

```ts
type RunningComfy = {
  pid: number
  name: string
  exePath: string | null
  cwd: string | null
  commandLine: string[]
  matchedInstallIds: string[]
  matchReason: 'exeUnderRoot' | 'cwdUnderRoot' | 'argUnderRoot'
  startedAt: string | null          // RFC 3339, UTC
  listeningPorts: number[] | null   // TCP ports it listens on
  holdsModelFiles: boolean | null   // holds a model file open
}
```

Returns `RunningComfy[]`.

A process matches an install when its executable, its working directory, or one
of its arguments sits under that install root.

The last three fields tell a working ComfyUI from a process that never exited.
For each of them, `null` means that the operating system gave no answer. `null`
never means "no".

- `startedAt` is when the process started.
- `listeningPorts` lists the TCP ports that the process listens on, on any
  local address. An empty list means that the process serves nothing: no
  person can open it in a browser. A ComfyUI that is still loading also listens
  on nothing, for a few seconds up to a few minutes. Use `startedAt` to tell a
  loading ComfyUI from one that started yesterday.
- `holdsModelFiles` tells whether the process holds open any model file that
  the vault knows about: every file that the last scan found, every place that
  a found link points to, and every file in the vault. Windows refuses to move
  a file that a program holds open, so this is the fact that blocks an apply.
  It is `null` before the first scan when the vault is empty, because then
  there is no file to ask about.

The engine never ends, pauses or signals a process. Ending one is the person's
action. `pid`, `exePath` and `startedAt` are there so that the person can find
the same process in Task Manager.

Data from a build before these fields existed reads back with all three set to
`null`.

To open a ComfyUI that listens, call the opener plugin's `open_url` with
`http://127.0.0.1:<port>/`, with the trailing slash, and the port written in
digits only. The window may open exactly that page, on any port. It may not
open another path, a query, a fragment, `https`, a user name before the host,
or any other address on this computer or the network.

### 11.2 `check_locked_files`

Arguments: `{ paths: string[] }`.

Returns:

```ts
type LockState = {
  path: string
  locked: boolean
  checkable: boolean       // false on systems without mandatory locking
  detail: string | null
}
```

Returns `LockState[]`.

On Windows the engine opens the file for writing with no sharing. A sharing
violation means another program holds it. That is the exact condition that makes
a move fail, so the check and the failure agree.

On Linux and macOS, `checkable` is `false` and `locked` is always `false`,
because those systems allow a file to be moved while it is open. The user
interface must not present `locked: false` as a guarantee when `checkable` is
`false`.

### 11.3 `open_task_manager`

Opens Windows Task Manager, where the person can end a process. The engine
never ends a process itself.

Arguments: none. Returns `null`.

It rejects with `ioError` when Windows does not open it. The message reads
"Windows did not open Task Manager. Press Ctrl+Shift+Esc to open it." On Linux
and macOS it rejects with `conflict`.

It answers before a vault is chosen.

---

## 12. Two things the person must be told

### 12.1 Developer Mode

Windows only creates symbolic links without administrator rights when Developer
Mode is on. The engine reports the real state in `PlatformReport`.

If `symlinks.supported` is `false`, the user interface must block Apply, and
must show `symlinks.guidance`, which reads:

> Windows needs Developer Mode to create the links this app uses. Open
> Settings, go to System, then For developers, and turn Developer Mode on. You
> do not need to restart.

### 12.2 Model thumbnails

ComfyUI version 0.28.0 added a security check to the route that serves model
preview thumbnails. That check rejects a file reached through a per-file
symbolic link.

The effect is limited to thumbnails in the ComfyUI model browser. Loading a
model is not affected. Running a workflow is not affected.

The engine reports each install's version. If an install runs 0.28.0 or newer,
the user interface must tell the person that model thumbnails will not load for
consolidated models, and that nothing else changes.

---

## 13. Event summary

| Event | Payload | Emitted by |
|---|---|---|
| `scan:progress` | `ScanProgress` | `start_scan` |
| `scan:done` | `ScanRecord` | `start_scan` |
| `scan:error` | `VaultError` | `start_scan` |
| `apply:progress` | `ApplyProgress` | `start_apply`, `resume_apply` |
| `apply:done` | `ApplyRecord` | `start_apply`, `resume_apply` |
| `apply:error` | `VaultError` | `start_apply`, `resume_apply` |
| `revert:progress` | `RevertProgress` | `revert_apply` |
| `revert:done` | `ApplyRecord` | `revert_apply` |
| `revert:error` | `VaultError` | `revert_apply` |
| `download:progress` | `Download` | `start_download`, `continue_download`, `stop_download`, and the transfer itself |

Subscribe before you call the command that starts the work:

```ts
import { listen } from '@tauri-apps/api/event'

const un = await listen<ScanProgress>('scan:progress', e => setProgress(e.payload))
const { scanId } = await invoke<{ scanId: string }>('start_scan', { args: {} })
```

---

## 14. Command index

| Command | Section |
|---|---|
| `get_platform_report` | 2.1 |
| `get_app_state` | 2.2 |
| `select_vault` | 2.3 |
| `get_settings` | 2.5 |
| `update_settings` | 2.5 |
| `validate_install_path` | 3.1 |
| `register_install` | 3.2 |
| `list_installs` | 3.3 |
| `refresh_install` | 3.4 |
| `update_install` | 3.5 |
| `unregister_install` | 3.6 |
| `list_install_model_dirs` | 3.7 |
| `start_scan` | 4.1 |
| `get_scan_entries` | 4.6 |
| `cancel_scan` | 4.7 |
| `get_last_scan` | 4.8 |
| `build_plan` | 5.1 |
| `get_plan` | 5.4 |
| `start_apply` | 6.1 |
| `cancel_apply` | 6.5 |
| `get_apply_result` | 6.6 |
| `list_applies` | 6.6 |
| `get_interrupted_applies` | 6.7 |
| `resume_apply` | 6.8 |
| `preview_revert` | 6.8 |
| `revert_apply` | 6.8 |
| `set_aside_run` | 6.7 |
| `create_link` | 7.1 |
| `remove_link` | 7.2 |
| `create_model_folder` | 7.3 |
| `list_links` | 7.4 |
| `list_vault_files` | 8.1 |
| `list_name_groups` | 8.4 |
| `set_canonical_name` | 8.5 |
| `remove_alias` | 8.6 |
| `list_orphans` | 8.7 |
| `delete_vault_file` | 8.8 |
| `check_vault_health` | 8.9 |
| `check_model_usage` | 9.1 |
| `get_metadata` | 10.1 |
| `fetch_metadata_batch` | 10.2 |
| `clear_metadata_cache` | 10.4 |
| `open_civitai_page` | 10.5 |
| `get_running_comfy` | 11.1 |
| `check_locked_files` | 11.2 |
| `open_task_manager` | 11.3 |
| `close_vault` | 2.3 |
| `get_vault_info` | 2.4 |
| `list_contents` | 8.3 |
| `remove_dangling_links` | 8.9 |
| `list_directory` | 15.1 |
| `create_directory` | 15.2 |
| `set_token` | 16.1 |
| `get_token_status` | 16.1 |
| `remove_token` | 16.1 |
| `read_model_address` | 16.2 |
| `start_download` | 16.3 |
| `stop_download` | 16.5 |
| `continue_download` | 16.5 |
| `discard_download` | 16.5 |
| `remove_download` | 16.5 |
| `list_downloads` | 16.5 |
| `open_huggingface_page` | 16.6 |

---

## 15. The folder picker

The interface lets the person choose a vault folder and an install folder by
browsing, because nobody types a path. These two commands exist so the browsing
goes through the engine, and the window needs no file system access of its own.

### 15.1 `list_directory`

Lists the folders inside one folder. Only folders come back, because the picker
only ever chooses a folder. Hidden folders are left out.

Arguments:

```ts
{ path?: string }      // absent or empty means EVERY drive
```

**With no path, the answer is every drive the computer has, not the contents of
`C:`.** The returned listing has an empty `path`, a `parent` of `null`, and one
entry per drive root. A person whose models live on `D:` has to be able to
reach them, and a picker that starts inside one drive can never leave it.

Going back up from a drive root returns `parent: ""`, which is the drive list
again.

Returns:

```ts
type DirectoryListing = {
  path: string
  parent: string | null
  entries: Array<{
    name: string
    path: string
    isDirectory: boolean
    isSymlink: boolean      // following it may leave the folder being browsed
  }>
}
```

A folder that cannot be read is an error carrying the path, not an empty list.

### 15.2 `create_directory`

Creates a folder the person named in the picker. This is how a new vault folder
is made.

Arguments: `{ path: string }`.

Returns: `{ path: string, created: boolean }`.

`created` is `false` when the folder was already there.

This command is deliberately not limited to an install, because a vault can
live anywhere. `select_vault` separately refuses a vault inside a registered
install. To create a folder **inside** an install, use `create_model_folder`
from section 7.3, which checks that the folder is somewhere ComfyUI reads.

---

## 16. Downloading a model

The person pastes the address of a model on Hugging Face or Civitai. The
engine reads it, the person confirms, and the engine downloads the file into
the vault and links it in the installs the person ticked.

Every download runs in the engine, never in the window. Nothing in the
window's own network rules changes.

### 16.1 Tokens

Some models download only for a signed-in account. Each person pastes their
own Hugging Face token and Civitai key. The engine keeps each one in Windows
Credential Manager, under this Windows account, on this computer, as the
generic credentials `ComfyVault/huggingface` and `ComfyVault/civitai`. A token
is never written to the vault, to a log, to an error, to an event or to the
journal, and no command returns it. A token is sent only to the site it
belongs to, in an `Authorization: Bearer` header, and never to the storage
address a site sends a download to.

These three commands answer before a vault is open.

#### `set_token`

Arguments: `{ service: 'huggingface' | 'civitai', token: string }`.

The engine asks the site first: Hugging Face `api/whoami-v2`, Civitai
`api/v1/me`. It saves the token only if the site accepts it.

Returns:

```ts
type TokenSaved = {
  ok: boolean                // always true
  account: string | null     // the Hugging Face account name. null for Civitai.
}
```

Errors:

| Code | When |
|---|---|
| `invalidArgument` | The text is empty, or is not one line of visible characters. |
| `conflict` | The site did not accept the token. `detail` is the site's own message. |
| `networkUnavailable` | The site did not answer. Nothing was saved. |
| `permissionDenied` | Credential Manager refused. |

#### `get_token_status`

Arguments: `{ service: 'huggingface' | 'civitai' }`.

```ts
type TokenStatus = {
  saved: boolean
  ok: boolean | null        // null: not saved, or the site did not answer
  account: string | null
  message: string | null    // the site's words when it refused, or why it was not checked
}
```

With a saved token, this asks the site each time.

#### `remove_token`

Arguments: `{ service: 'huggingface' | 'civitai' }`. Returns `{ removed: true }`.

### 16.2 `read_model_address`

Reads an address and says what a download would do. Nothing is downloaded.

Arguments:

```ts
{
  address: string
  versionId?: number     // Civitai: another version than the one chosen
  fileId?: number        // Civitai: another file of the version
  category?: string      // the folder the person chose in the menu
}
```

The addresses it reads:

- `huggingface.co/{owner}/{repo}/blob/{revision}/{path}` and
  `.../resolve/{revision}/{path}`, with or without `?download=true`.
- `civitai.com/models/{id}` and `civitai.com/models/{id}/{slug}`, both with
  or without `?modelVersionId={v}`, and `civitai.com/api/download/models/{v}`.

Anything else is refused before any request. So is a path part that decodes
to a folder separator.

Returns one of two branches, never both:

```ts
type AddressReading = {
  plan: AddressPlan | null
  refusal: AddressRefusal | null
}
```

```ts
type AddressRefusal = {
  kind: 'badAddress' | 'hfRepoNotFile' | 'tokenMissing' | 'tokenRejected' | 'noAccess' | 'notFound'
  host: 'huggingface' | 'civitai' | null     // null for badAddress
  serviceMessage: string | null              // the site's own words, verbatim
  page: HfPage | null                        // Hugging Face: for "Open the model's page"
}
```

`serviceMessage` is text from the network. Show it as text, never as markup.

How a site's answer becomes a `kind`:

| The site answers | `kind` |
|---|---|
| Hugging Face 401, no token saved | `tokenMissing` |
| Hugging Face 401, with a token | `tokenRejected` |
| Hugging Face 403 | `noAccess`, or `tokenMissing` with no token saved |
| Hugging Face `x-error-code` `RepoNotFound`, `EntryNotFound` or `RevisionNotFound`, whatever the status | `notFound` |
| Civitai 401, no token saved | `tokenMissing` |
| Civitai 401, with a token | `tokenRejected` |
| Civitai 403 | `noAccess`, or `tokenMissing` with no token saved |
| 404 on either site | `notFound` |

The engine finds a refusal while reading the address. It asks for the file
itself with the saved token, and does not follow the redirect.

```ts
type AddressPlan = {
  host: 'huggingface' | 'civitai'
  title: string                 // the Civitai model's name, or the Hugging Face file name
  subtitle: string              // the Civitai model's address, or the Hugging Face owner/repo
  versions: VersionChoice[]     // Civitai only, newest first
  versionId: number | null
  files: FileChoice[]           // Civitai only, the primary file first
  fileId: number | null
  fileName: string
  sizeBytes: number
  sha256: string | null         // uppercase; null for a small Hugging Face file stored without LFS
  suggestedCategory: string | null
  suggestedBecause: string | null
  category: string | null       // the folder this plan was worked out for
  categories: string[]          // the folder menu
  alreadyInVault: AlreadyInVault | null
  vaultRelPath: string | null   // null while category is null
  vaultNameTaken: boolean
  installs: InstallTarget[]
  vaultFreeBytes: number | null
  spaceNeededBytes: number
  page: HfPage | null
  modelId: number | null
}

type VersionChoice = { id: number, name: string }
type FileChoice = { id: number, name: string, sizeBytes: number, detail: string }
type HfPage = { owner: string, repo: string }
type AlreadyInVault = {
  vaultRelPath: string
}

type InstallTarget = {
  installId: string
  linkPath: string | null       // null while category is null, and for an unavailable install
  state: 'free' | 'hasLink' | 'nameTaken' | 'unavailable'
  ticked: boolean
}
```

- `sizeBytes` is exact for Hugging Face. Civitai states sizes in kilobytes, so
  for Civitai it is close. The transfer uses the exact size the storage sends.
- `suggestedCategory` comes from Civitai's `type` (`Checkpoint` goes to
  `checkpoints`; `LORA`, `LoCon` and `DoRA` to `loras`; `TextualInversion` to
  `embeddings`; `VAE` to `vae`; `Controlnet` to `controlnet`; `Upscaler` to
  `upscale_models`), or from the deepest folder in the Hugging Face path that
  names a category. Anything else suggests nothing.
- `category` is the argument, or else `suggestedCategory`. When it is null,
  `vaultRelPath` and every `linkPath` are null, and a download cannot start.
- `categories` holds ComfyUI's own categories and every folder the vault
  already has, sorted.
- `vaultNameTaken` says that a different model already has this name in this
  vault folder. With a known SHA-256, `vaultRelPath` already carries the tag,
  for example `checkpoints\dreamshaper_8__879DB523.safetensors`. Without one,
  `vaultRelPath` shows the plain name, and the tag is added when the file is
  in hand. A link always keeps the plain name.
- `alreadyInVault`: the vault holds a file with this SHA-256. Nothing will be
  downloaded, and `spaceNeededBytes` is 0.
- `spaceNeededBytes` is `sizeBytes` plus 5,000,000,000 bytes kept free on the
  vault's drive. `start_download` refuses below it.
- `ticked`: the first time, every install whose `state` is `free`. After
  that, the installs ticked for the last download. An install with state
  `hasLink` is always ticked. An install with state `nameTaken` or
  `unavailable` is never ticked.

#### Where a link goes

This follows ComfyUI's own `folder_paths.py` and `utils/extra_config.py`,
read on 2026-09-26:

- Each built-in category starts with a fixed list of folders under `models`.
  `diffusion_models` lists `models\unet` first, then
  `models\diffusion_models`. `text_encoders` lists `models\text_encoders`,
  then `models\clip`. `controlnet` lists `models\controlnet`, then
  `models\t2i_adapter`.
- `extra_model_paths.yaml` is read in file order. A folder marked
  `is_default: true` is put first in its category's list, so the last such
  folder in the file ends up first. Any other folder is added at the end.
- The output model folders are added after that.
- To load a model by name, ComfyUI takes the first folder in the list that
  holds a file of that name.

So:

- **The link goes where ComfyUI saves new files of that category.** That is
  the folder `extra_model_paths.yaml` marks `is_default: true` for the
  category, if there is one, and otherwise `models\{category}`. A category
  that ComfyUI does not know itself, and that only the YAML file names, goes
  into the first folder the YAML file gives it. A YAML folder that contains
  the whole install is never used; the link then goes into
  `models\{category}`.
- **A name is taken when any folder ComfyUI searches for that category, in
  that install, already holds it.** One of the two files would never load,
  wherever the link went. That install's `state` is `nameTaken`, and nothing
  in it is touched.
- Two installs whose YAML files name the same shared folder get the same
  `linkPath`. One link serves both.

### 16.3 `start_download`

Arguments:

```ts
{
  address: string
  versionId?: number
  fileId?: number
  category: string
  installIds: string[]     // may be empty: the file is then only in the vault
}
```

Returns a `Download`.

The engine reads the address again, so this takes a few seconds, and it can
refuse as `read_model_address` can. A refusal comes back as `conflict`, with
the site's words in `detail`. The engine also refuses with `ioError` when the
vault's drive has less free space than `spaceNeededBytes`, and with
`notFound` for an install that is not registered.

With the model already in the vault, the engine makes the links at once,
transfers nothing, and returns the download in state `linkedOnly`.

Otherwise the download is queued. One transfer runs at a time, in the order
they were started. A download runs beside a scan, a consolidation and an
undo, and does not take their place. None of them ever writes over a file,
so a name that one of them takes first makes the other end with a clear
message rather than overwrite it.

### 16.4 A download

```ts
type Download = {
  downloadId: string
  host: 'huggingface' | 'civitai'
  title: string
  fileName: string
  category: string
  vaultRelPath: string          // where it goes, and in the end where it went
  installIds: string[]          // the installs to link
  linkedInstallIds: string[]    // the installs linked, once done or linkedOnly
  notLinked: NotLinked[]
  alreadyInVault: boolean
  state: 'waiting' | 'running' | 'checking' | 'stopped' | 'failed' | 'mismatch' | 'cutOff' | 'done' | 'linkedOnly'
  bytesDone: number
  bytesTotal: number
  bytesPerSecond: number | null // over the last 5 seconds; null until known
  error: DownloadError | null
  startedAt: string | null
  finishedAt: string | null
}

type NotLinked = {
  installId: string
  reason: string                // one sentence to show as it is
}

type DownloadError = {
  kind: 'connection' | 'refused' | 'expired' | 'noSpace' | 'mismatch' | 'disk' | 'changedOnSite'
  message: string               // one sentence for the person
  serviceMessage: string | null // the site's own words, verbatim
  detail: string | null         // what the connection or the disk reported, for a details panel
}
```

What happens, in order:

1. **Running.** The engine asks the site's own address again every time, since
   a signed storage address expires, and never reuses an old one. It follows
   the redirect itself, without the token. The bytes go into a part file in
   the vault's own `.comfyvault\downloads` folder, on the vault's drive. A kept
   part continues with an HTTP `Range` request, and `If-Range` names the
   version it came from. A file that changed on the site is then sent whole,
   and the old part is dropped, never joined to the new one.
2. **Checking.** The whole file is hashed. If it does not match the SHA-256 the
   site gave, the file is deleted, nothing goes into the vault, nothing is
   linked, and the state is `mismatch`. With no SHA-256 known, the hash just
   computed is used. If the vault already holds that content, the new file is
   deleted, the links go to the file already there, and `alreadyInVault` is
   true.
3. **Done.** The file is renamed into the vault, never over anything. Then each
   ticked install gets its link, as section 16.2 says. Each step is written to
   the journal before it happens, under the run name `download-{downloadId}`,
   which is never a run to finish or undo. An install that cannot take its
   link at the end, for example because a file of that name appeared there,
   is listed in `notLinked`. The model stays in the vault, and the state is
   still `done`.

**No silent retry.** A dropped line, a line silent for 30 seconds, an expired
storage address, a 401 or 403 in the middle, and a full drive each end the
attempt as `failed`, with its `kind`, and the part is kept. The engine does not
try again on its own. `continue_download` tries again.

`changedOnSite`: the site now gives a different SHA-256 for this file than
when the download started. Discard it and read the address again.

#### After the app closes

A download that was `waiting`, `running` or `checking` when the app closed
comes back as `cutOff`, with `bytesDone` read from its part. Nothing starts
by itself. `stopped` and `failed` downloads stay as they were. `done`,
`linkedOnly` and `mismatch` downloads are gone from the list, since nothing
of them is left on disk to keep.

A download cut off after its file went into the vault finishes when it is
continued: the engine finds the file where the journal says, checks its
SHA-256, and makes the record and the links.

`close_vault`, and `select_vault` for another vault, stop the transfer first,
and the part is kept. The download shows as `stopped` when the vault opens
again.

### 16.5 Commands on a download

All take `{ downloadId: string }`.

| Command | Returns | Allowed when | Does |
|---|---|---|---|
| `stop_download` | `Download` | `waiting`, `running`, `checking` | Stops it at once. The part is kept. |
| `continue_download` | `Download` | `stopped`, `failed`, `cutOff`, `mismatch` | Queues it again. It reads the address again and carries on from the part. |
| `discard_download` | `{ removed: true }` | not `waiting`, `running` or `checking` | Deletes the part, and removes the download from the list. |
| `remove_download` | `{ removed: true }` | `done`, `linkedOnly`, `mismatch`, or `waiting` with no part | Removes it from the list. |

A command used in another state rejects with `conflict`. `list_downloads`
takes no argument and returns `Download[]`, in the order they were started.

A download that was stopped a moment ago can still be letting go of its
connection. Until it has, which can take up to 30 seconds on a line that
went silent, `continue_download` and `discard_download` reject it with
`conflict` and the message "That download is still stopping. Try again in a
moment." or "Stop that download first, then discard it.".

### 16.6 `open_huggingface_page`

Arguments: `{ owner: string, repo: string }`. Returns `null`.

Opens `https://huggingface.co/{owner}/{repo}` in the default browser. The
window sends the two names, never an address, and the engine refuses a name
that is not a plain Hugging Face name, with `invalidArgument`. It answers
before a vault is open.

### 16.7 `download:progress`

The payload is a `Download`. It is sent on every change of state, and at most
4 times a second while bytes move.
