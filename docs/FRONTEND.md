# The ComfyVault interface

Solid and TypeScript, built by Vite, drawn into the Tauri window.

This document is for working on the interface itself. To build the whole
application, see [BUILD.md](BUILD.md).

## Run it

Node 22.13 or newer is required. Node 20 cannot install this dependency set.

```
npm install
npm run dev        # http://localhost:1420
npm test           # the whole suite
npm run build      # type check, then dist/
```

`npm run dev` opens the interface in a browser against the development engine
in `src/ipc/fixture/`. That engine follows the rules in `docs/IPC-CONTRACT.md`
and returns the same shapes the Rust engine returns. It reads nothing from disk
and never sees a real ComfyUI install.

Two things only Windows can really do, so the browser build exposes them on the
console:

```js
comfyVaultDev.symlinks(true)      // Developer Mode on, so links can be made
comfyVaultDev.comfyRunning(false) // close ComfyUI
comfyVaultDev.reset(false)        // start over; pass true for a first run
comfyVaultDev.breakLinks(1)       // point some links at nothing, for Cleanup
comfyVaultDev.forgetVersions()    // installs that do not record a version
comfyVaultDev.noWorkflows()       // no saved workflow files to search
```

`http://localhost:1420/?first-run` opens it with nothing registered.

Inside the Tauri window the same interface talks to the Rust engine. Nothing in
the fixture is loaded there: `src/ipc/client.ts` picks one or the other, and
each is its own chunk.

## What the pieces are

```
src/ipc/contract.ts      every shape the engine speaks, from docs/IPC-CONTRACT.md
src/ipc/tauri.ts         the real engine: one method per command
src/ipc/fixture/         the development engine, and the world it answers from
src/domain/              pure functions: the view model, the totals, the wording
src/state/store.ts       what the interface knows, and the memos over it
src/screens/             one file per screen
src/modals/              the folder picker and the confirmations
src/styles/app.css       the design system
```

No screen talks to Tauri. Every screen reads the port in `src/ipc/contract.ts`.

## What the Rust side has to provide

Beyond the commands in `docs/IPC-CONTRACT.md`:

- `tauri-plugin-opener`, with `opener:allow-open-url` and
  `opener:allow-reveal-item-in-dir`. Used to open the Windows Developer Mode
  settings page, a Civitai model page, a ComfyUI at `http://127.0.0.1:<port>/`,
  and to show the vault folder in Explorer.
- Nothing else. The folder picker browses through `list_directory` and
  `create_directory` from section 15 of the contract, so the window needs no
  file system access of its own.
- A window of 1000 x 660 with `decorations: false`. The interface draws its own
  title bar, and the drag region is already marked.
