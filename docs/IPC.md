# EdgeHop core IPC (v1)

How a user interface talks to the **core**: a headless helper process (`edgehop-core` on macOS, the Rust core on
Windows) that owns input capture, injection and the network link. The core implements the wire protocol in
[`PROTOCOL.md`](PROTOCOL.md); this document is only the local UI to core API.

## Transport
* The UI spawns the core as a child process and talks over its **stdin / stdout**.
* **Newline-delimited JSON**: one UTF-8 JSON object per line, in both directions. No other output is ever
  written to stdout; diagnostics go to stderr.
* Closing the core's stdin (the UI exited or crashed) makes it stop all sessions, give the cursor back and exit.
  There is nothing to authenticate: only the parent process holds the pipe.

## Messages
Request (UI to core):

```json
{"id": 7, "method": "controller.start", "params": {"token": "...", "peer": "MacBook Pro"}}
```
`id` is a number or a string chosen by the UI. `params` may be omitted when a method takes none.

Response (core to UI), exactly one per request, echoing `id`:

```json
{"id": 7, "result": {}}
{"id": 7, "error": {"code": "invalid_params", "message": "token must be at least 12 characters"}}
```

Event (core to UI), unsolicited, no `id`:

```json
{"event": "controller.state", "data": {"state": "connected", "peer": "MacBook Pro", "remoteActive": true}}
```

Error codes: `parse_error` (line is not JSON, `id` is `null`), `invalid_request` (no usable `method`),
`unknown_method`, `invalid_params`, `busy` (already running), `internal`.

## Methods

| Method | Params | Result |
|---|---|---|
| `core.hello` | none | `{name, version, ipcVersion: 1, platform, deviceName}` |
| `core.shutdown` | none | `{}`, then the core stops everything and exits |
| `permissions.get` | none | `{accessibility: bool, inputMonitoring: bool}` |
| `permissions.request` | `{kind: "accessibility" \| "inputMonitoring"}` | `{}` (shows the system prompt) |
| `token.generate` | none | `{token}` (20 characters) |
| `peers.watch` | `{enabled: bool}` | `{}`; while enabled the core emits `peers.changed` |
| `peers.list` | none | `{peers: [{name}]}` |
| `controller.start` | see below | `{}`; progress arrives as `controller.state` events |
| `controller.stop` | none | `{}` |
| `controller.state` | none | state object |
| `receiver.start` | `{token, allowKeys?: false, debug?: false, port?: 47474}` | `{port}` |
| `receiver.stop` | none | `{}` |
| `receiver.state` | none | state object |

`controller.start` params: `token` (at least 12 characters), exactly one of `peer` (a name from `peers.list`)
or `host` (address, no `user@`), optional `port` (47474), `side` (`left`/`right`/`top`/`bottom`, default `right`:
where the other computer sits), `switching` (default `true`; `false` mirrors the mouse on both),
`forwardKeys` (default `false`), `debug` (default `false`).

## Events

| Event | Data |
|---|---|
| `controller.state` | `{state: "idle" \| "connecting" \| "connected" \| "failed", peer?, remoteActive?, message?}` |
| `receiver.state` | `{state: "stopped" \| "listening" \| "connected", port?, peer?}` |
| `controller.warning` | `{message}`: the other computer connected but cannot apply input (for example a Mac without the Accessibility permission). Shown while connected; the UI clears it when the session ends |
| `latency` | `{ms}` (controller ping round trip, every 2 s) |
| `log` | `{source: "controller" \| "receiver", line}` |
| `peers.changed` | `{peers: [{name}]}` |

`remoteActive` is `true` while this computer's keyboard and mouse are driving the other computer.
`failed` carries a human-readable `message`; the UI can show it as is.

## Rules for implementers
* Handle every request; never leave a request without a response.
* Answer `core.hello` without side effects (no prompts, no network).
* Start discovery only after `peers.watch {enabled: true}` (it triggers the macOS Local Network prompt).
* Events may arrive between a request and its response; match responses by `id`.
* Check a core with `scripts/ipc-smoke.mjs` (real client) and `scripts/ipc-conformance.mjs` (differential: two cores
  must give identical responses to the same requests).
* Keep the same names on every platform. Platform differences go in `core.hello.platform` and in which
  permissions exist (on Windows `permissions.get` returns both as `true`).
