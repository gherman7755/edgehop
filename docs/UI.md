# Desktop UI (Electron)

`ui/` is the EdgeHop front end for macOS and Windows: an Electron app that drives a **core** process over the
stdin/stdout API in [`IPC.md`](IPC.md). It has no input or network code of its own, so the same screens work with
the Swift core (macOS) and the Rust core (Windows).

## Run it

```bash
# 1. a core (the UI looks for these, release before debug, or for EDGEHOP_CORE)
swift build --product edgehop-core                    # macOS: .build/debug/edgehop-core
(cd rust && cargo build -p edgehop-core)              # Windows: rust/target/debug/edgehop-core.exe

# 2. the UI (Node 20.19+ or 22.12+)
cd ui
npm ci
npm run dev                                           # the app, with hot reload
npm run preview:web                                   # only the screens, in a browser, with a simulated core
```
`preview:web` (http://localhost:5199) is for working on the design: `?platform=windows`, `?peers=1`, `?core=exited`
and `?granted=1` simulate the other situations, and `window.__mock` lets you push events by hand.

## How it fits together

| Part | Where | Role |
|---|---|---|
| Core client | `src/main/core/client.ts` | NDJSON over the core's pipes: requests matched to answers by `id`, events, timeouts, a clean `closed` error. Knows nothing about processes. |
| Supervisor | `src/main/core/supervisor.ts` | Finds and starts the core, checks `core.hello` (IPC version 1), reports `starting` / `running` / `exited`, and on quit asks the core to shut down (closing its stdin releases the keyboard and mouse even if that fails). |
| Main process | `src/main/index.ts` | Window, single-instance lock, tray, settings; forwards a **whitelist** of requests from the page to the core. |
| Preload | `src/preload/index.ts` | The only door between the page and Electron: `window.edgehop` (request, events, settings, addresses). |
| Screens | `src/renderer/src` | React. `state.ts` (reducer) and `derive.ts` (what the header says, why Connect is disabled) are pure and unit-tested. |

Behaviour worth knowing:

* Closing the window while sharing keeps EdgeHop running in the menu bar / notification area; its menu shows the
  status and can stop sharing. Quitting stops everything first.
* Tokens are stored encrypted with the operating system's keychain (Electron `safeStorage`). If that is not
  available they are kept for the run only, and the screens say so.
* The page has context isolation, a sandbox, a strict content security policy and cannot navigate away.
* Keyboard sharing switches are disabled for cores that do not have the neutral key map (the macOS and Windows cores both have it).

## Tests

```bash
cd ui
npm run typecheck
npm test            # unit tests, a fake core, and the real core when one is built
EDGEHOP_SMOKE=1 EDGEHOP_USER_DATA=$(mktemp -d) npx electron .    # after `npm run build`: the whole app, headless check
```
CI runs all of this on macOS (Swift core) and Windows (Rust core); the smoke run launches the real Electron app,
waits for the page to show a running core and asks it for a token through the preload bridge.
