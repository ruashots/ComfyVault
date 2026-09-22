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
| File path | String. On Windows the separator is a backslash. |
| Identifier | String. Format is UUID v4 unless this document says otherwise. |
| Hash | 64 hexadecimal characters, uppercase. The algorithm is SHA-256. |

### 1.3 Errors

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

### 1.4 Concurrency rule

The engine runs one long operation at a time. A scan, an apply, and a revert
are long operations. If a second long operation starts, the engine rejects it
with `vaultBusy`. All other commands stay callable during a long operation.

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

### 2.2 `get_app_state`

Arguments: none.

Returns:

```ts
type AppState = {
  vaultRoot: string | null
  vaultInitialized: boolean
  installCount: number
  platform: PlatformReport
  settings: Settings
  lastScanId: string | null
  lastPlanId: string | null
  interruptedApplies: string[]   // apply identifiers that need recovery
  busy: null | { kind: 'scan' | 'apply' | 'revert', id: string }
}
```

Call this command when the application starts. If `interruptedApplies` is not
empty, the user interface must show the recovery screen before anything else.

### 2.3 `select_vault`

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
  volume: string          // 'C:' on Windows, the mount point on Linux
  freeBytes: number
  totalBytes: number
  fileCount: number
  totalStoredBytes: number
  schemaVersion: number
}
```

Errors: `ioError`, `permissionDenied`, `storeError`, `invalidArgument`.

The engine refuses a vault path that sits inside a registered install. That
refusal uses `conflict`.

### 2.4 `get_settings` and `update_settings`

```ts
type Settings = {
  metadataLookupsEnabled: boolean   // default true
  civitaiApiKey: string | null      // optional. Never logged.
  hashCacheEnabled: boolean         // default true
  scanExtensions: string[]          // default list is in section 4.2
  minFileSizeBytes: number          // default 1048576
  followExtraModelPaths: boolean    // default true
  scanOutputModelDirs: boolean      // default true
}
```

`update_settings` takes a partial object. Every field is optional. The engine
applies only the fields that are present.

`get_settings` never returns the API key value. It returns `"***"` when a key
is stored and `null` when no key is stored.

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
  extraPathsError: string | null
  outputModelDirs: string[]
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
  relPath: string        // relative to the root that owns it, for example 'loras/style'
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
5. The Hugging Face cache. These files are counted. They are never moved. The
   engine looks at `HF_HOME`, then `HUGGINGFACE_HUB_CACHE`, then
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
| `scan:done` | `ScanResult` |
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

### 4.5 `ScanResult`

```ts
type ScanResult = {
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
  entries: ScanEntry[]
}

type ScanEntry = {
  absPath: string
  relPath: string
  installId: string
  category: string
  sizeBytes: number
  sha256: string | null
  modifiedAt: string
  classification: Classification
  occurrenceCount: number        // how many paths hold these bytes
  linkTarget: string | null      // set when the entry is already a link
}

type Classification =
  | 'movable'
  | 'customNodes'
  | 'huggingFaceCache'
  | 'alreadyInVault'
  | 'externalLink'
  | 'unreadable'
```

### 4.7 `cancel_scan`

Arguments: `{ scanId: string }`. Returns `{ cancelled: true }`.

A cancelled scan emits `scan:done` with `cancelled` set to `true`. A cancelled
scan changes nothing on disk. Hashes already computed stay in the cache.

### 4.8 `get_last_scan`

Arguments: none. Returns `ScanResult | null`.

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

`build_plan` still succeeds when symbolic links are not supported. It marks
every row as blocked with the reason `symlinkUnsupported`, so the person can
read the plan before they turn Developer Mode on.

### 5.2 `ConsolidationPlan`

```ts
type ConsolidationPlan = {
  planId: string
  scanId: string
  createdAt: string
  vaultRoot: string
  groups: PlanGroup[]
  blocked: BlockedRow[]
  totals: PlanTotals
}

type PlanGroup = {
  groupId: string
  sha256: string
  sizeBytes: number
  category: string
  vaultRelPath: string           // for example 'loras/lora1.safetensors'
  vaultNameAdjusted: boolean
  clashesWith: string | null     // the SHA-256 that already owns the plain name
  source: PlanSource
  links: PlanLink[]
  occurrences: number
  bytesFreed: number
  singleCopy: boolean
  crossVolume: boolean
}

type PlanSource = {
  installId: string
  installLabel: string
  absPath: string
  relPath: string
  sameVolumeAsVault: boolean
  chosenBecause: 'sameVolume' | 'onlyCopy' | 'firstByPath'
}

type PlanLink = {
  installId: string
  installLabel: string
  absPath: string
  relPath: string
  linkName: string               // the name the link keeps
  nameDiffersFromVault: boolean
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
  vaultFreeBytesAfter: number
}
```

### 5.3 How the plan decides

**One group per unique content.** Every file with the same SHA-256 belongs to
one group, whatever its name and whatever folder it sits in. The person's case
is the normal case: the same weight file under `loras\awesomeloras\` in one
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

**The link keeps its own name.** A link is always created with the name the
file had in that install. The vault file name can differ. When it differs,
`nameDiffersFromVault` is `true`.

**Which copy becomes the vault file.** The engine prefers a copy that already
sits on the vault volume, because that move is a rename and takes no time and
no extra space. If no copy sits on the vault volume, the engine takes the
first copy by sorted path. `chosenBecause` reports the rule that fired.

**Single copies.** A file that exists once still moves into the vault. It
frees nothing. `singleCopy` is `true` and `bytesFreed` is `0`. The user
interface must present these separately, because the person expects the space
number to match the row count.

**Blocked rows.** A row that cannot move never appears in a group. It appears
in `blocked` with a reason.

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
| `apply:done` | `ApplyResult` |
| `apply:error` | `VaultError` |

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
  bytesToMove: number
  bytesFreed: number
  filesMoved: number
  linksCreated: number
  failures: number
  elapsedMs: number
  etaMs: number | null
}
```

### 6.4 `ApplyResult`

```ts
type ApplyResult = {
  applyId: string
  planId: string
  state: 'completed' | 'completedWithErrors' | 'cancelled' | 'interrupted' | 'reverted'
  startedAt: string
  finishedAt: string | null
  groupsRequested: number
  groupsApplied: number
  groupsFailed: number
  bytesFreed: number
  filesMoved: number
  linksCreated: number
  failures: ApplyFailure[]
  revertible: boolean
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

The engine finishes the group it is working on, then stops. It does not
interrupt a group. The result reports `cancelled`. Everything already applied
stays applied, and stays revertible.

### 6.6 `get_apply_result` and `list_applies`

`get_apply_result` takes `{ applyId: string }` and returns `ApplyResult`.

`list_applies` takes no arguments and returns `ApplyResult[]`, newest first.

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
}
```

Returns `InterruptedApply[]`. If the list is not empty, the user interface must
resolve it before it allows a new scan or a new apply.

### 6.8 `resume_apply` and `revert_apply`

`resume_apply` takes `{ applyId: string }` and finishes an interrupted run. It
re-checks every file it has not yet touched. It emits the apply events.

`revert_apply` takes `{ applyId: string }` and undoes a run, in reverse order.
It emits `revert:progress` and `revert:done`, which carry the same payloads as
the apply events.

A revert restores every original file. When the original bytes were deleted,
the engine copies them back from the vault, because the content is identical by
hash. A revert therefore needs free space on the install volume. The engine
checks that space first, and rejects with `notEnoughSpace` if it is short.

`revert_apply` rejects with `conflict` when a later apply depends on the run.

---

## 7. Links

### 7.1 `create_link`

Creates one symbolic link inside an install, pointing at a vault file.

Arguments:

```ts
{
  installId: string
  sha256: string
  relativeDir: string        // relative to the install root, for example 'models/loras/style'
  linkName?: string          // defaults to the vault file name
  createDir: boolean         // default false
}
```

Returns a `Link`.

```ts
type Link = {
  id: string
  installId: string
  absPath: string
  relPath: string
  linkName: string
  sha256: string
  vaultRelPath: string
  createdAt: string
  createdBy: 'apply' | 'manual'
  state: 'ok' | 'dangling' | 'replaced' | 'missing'
}
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
{ installId?: string, sha256?: string, state?: Link['state'] }
```

Returns `Link[]`.

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
  links: Link[]
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

### 8.3 `list_name_groups`

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

### 8.4 `set_canonical_name`

The person picks the name the vault keeps as the real file.

Arguments: `{ sha256: string, name: string }`.

Returns the updated `VaultFile`.

The engine makes the chosen name the real file and turns the previous real name
into an in-vault link. It then repoints every install link to the new real
path, so no link resolves through a second link. Every step is journaled and
revertible.

### 8.5 `remove_alias`

Removes one in-vault name. This is a separate action, on purpose. Names stay by
default, so saved workflows keep working.

Arguments: `{ sha256: string, name: string }`.

Returns `{ removed: true }`.

The engine rejects with `conflict` if the name is the canonical name. It
rejects with `conflict` if any install link resolves through that name, and
`detail` lists the links.

### 8.6 `list_orphans`

Lists vault files that no install links to.

Arguments: none. Returns `VaultFile[]` where `linkCount` is `0`.

The engine verifies each recorded link on disk before it answers. A link that
disappeared no longer counts.

### 8.7 `delete_vault_file`

Deletes a vault file and every name it carries. **This action cannot be
reverted.** The bytes are gone.

Arguments:

```ts
{ sha256: string, confirm: string }    // confirm must equal the sha256
```

Returns `{ deleted: true, bytesFreed: number }`.

The engine rejects with `conflict` when any install link points at the file.
Remove the links first.

The user interface must ask the person to confirm, and must say that the action
cannot be undone.

### 8.8 `check_vault_health`

Arguments: none.

Returns:

```ts
type VaultHealth = {
  checkedLinks: number
  checkedFiles: number
  danglingLinks: Link[]          // the link exists, the target does not
  replacedLinks: Link[]          // a real file sits where a link belonged
  missingVaultFiles: VaultFile[] // recorded in the database, absent on disk
  foreignFiles: string[]         // files in the vault folder the database does not know
  ok: boolean
}
```

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
  matches: UsageMatch[]
  method: string        // always the sentence below
}

type UsageMatch = {
  installId: string
  installLabel: string
  workflowPath: string
  workflowName: string
}
```

Returns `UsageResult[]`, one per requested name.

`method` is always this sentence:

> The file name was searched for as plain text inside saved workflow files.

**This check is deliberately shallow.** The engine looks for the file name as
text inside the JSON. It does not parse the graph. It does not resolve node
inputs. A match means the name appears. It does not prove the model runs.

The user interface must show the `method` sentence next to the result. A person
must never read "not used" as "safe to delete" without being told what the
check actually did.

The engine searches these locations under each install root:

```
user/<any user>/workflows/**/*.json
user/<any user>/subgraphs/**/*.json
any file named workflow.json, outside models, custom_nodes, .git, and virtual environments
```

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
  previewImageUrls: string[]
  ambiguous: boolean               // the hash matched more than one model version
}
```

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
}
```

Returns `RunningComfy[]`.

A process matches an install when its executable, its working directory, or one
of its arguments sits under that install root.

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
| `scan:done` | `ScanResult` | `start_scan` |
| `scan:error` | `VaultError` | `start_scan` |
| `apply:progress` | `ApplyProgress` | `start_apply`, `resume_apply` |
| `apply:done` | `ApplyResult` | `start_apply`, `resume_apply` |
| `apply:error` | `VaultError` | `start_apply`, `resume_apply` |
| `revert:progress` | `ApplyProgress` | `revert_apply` |
| `revert:done` | `ApplyResult` | `revert_apply` |
| `revert:error` | `VaultError` | `revert_apply` |

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
| `get_settings` | 2.4 |
| `update_settings` | 2.4 |
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
| `revert_apply` | 6.8 |
| `create_link` | 7.1 |
| `remove_link` | 7.2 |
| `create_model_folder` | 7.3 |
| `list_links` | 7.4 |
| `list_vault_files` | 8.1 |
| `list_name_groups` | 8.3 |
| `set_canonical_name` | 8.4 |
| `remove_alias` | 8.5 |
| `list_orphans` | 8.6 |
| `delete_vault_file` | 8.7 |
| `check_vault_health` | 8.8 |
| `check_model_usage` | 9.1 |
| `get_metadata` | 10.1 |
| `fetch_metadata_batch` | 10.2 |
| `clear_metadata_cache` | 10.4 |
| `get_running_comfy` | 11.1 |
| `check_locked_files` | 11.2 |
