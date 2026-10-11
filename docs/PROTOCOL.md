# Flint provider protocol, version 1

How Flint talks to the processes that give it items and carry out actions,
and how command-line clients talk to the Flint daemon. Implemented in
`flint-protocol`; this document is its specification. When the two
disagree, fix one of them in the same change.

## Transport

**Framing.** Every message is a UTF-8 JSON body preceded by a header,
exactly as LSP and DAP frame theirs:

```
Content-Length: 87\r\n
\r\n
{"jsonrpc":"2.0","id":1,"method":"hello","params":{...}}
```

The format is byte-for-byte what `fenix-rpc`'s `read_message`/
`write_message` handle, so Flint, Fenix's language-server client and
Fenix's control protocol share one framing. Maximum body size: 4 MiB;
header lines are capped at 1 KiB and a message at 16 header lines. A
larger value is a protocol error: the host answers with an error whose
`id` is `null` and closes the connection, **before** any allocation of
that size.

`fenix-rpc::read_message` allocates whatever `Content-Length` says and
reads header lines without a bound, which is fine for a language server
Fenix chose to spawn but not for a socket other processes can reach. Until
it gains a limit (F16 in FENIX-INTEGRATION.md), `flint-protocol` uses
`fenix_rpc::write_message` and its own bounded reader with the same
semantics (clean EOF between messages is `Ok(None)`, truncation is an
error, unknown headers are ignored), tested against `fenix-rpc`'s.

**Envelope.** JSON-RPC 2.0: requests (`id` + `method`), responses
(`id` + `result` or `error`), notifications (`method`, no `id`). Either
side may send requests; ids are per-direction.

**Channels.**

| Channel | Who starts it | Used by |
|---|---|---|
| stdio | Flint spawns the provider from `[[providers]] command`; Flint writes to its stdin and reads its stdout; stderr goes to Flint's log | most providers |
| control endpoint | the other process connects to Flint's per-user pipe/socket (see DESIGN.md, Security) | the `flint` CLI client; long-running apps acting as providers (`connect = true`) |

## Roles and handshake

There are two roles besides the host (Flint itself): **provider** and
**client**. On every channel, the non-host side speaks first.

### `hello` (request, peer → host)

```json
{
  "protocol": 1,
  "role": "provider",
  "name": "jira",
  "version": "0.3.0",
  "pid": 4120
}
```

Response:

```json
{
  "protocol": 1,
  "host": { "name": "flint", "version": "0.1.0", "pid": 9932 }
}
```

- `protocol` mismatch: the host answers with error `-32001
  UnsupportedProtocol` whose `data` is `{ "supported": [1] }`, then closes.
- A peer connecting on the control endpoint with `role: "provider"` must
  match a `[[providers]]` entry with `connect = true` and that `name`;
  otherwise error `-32003 NotDeclared` and close. Spawned providers are
  already declared.
- The `pid` lets Flint grant focus to the peer on Windows
  (`AllowSetForegroundWindow`) before asking it to show something.

Nothing else may be sent before the `hello` response.

## Provider messages

### `provider/register` (request, provider → host)

Sent once after `hello`. Declares what the provider is.

```json
{
  "id": "jira",
  "title": "Jira",
  "prefix": "j",
  "dynamic": true,
  "debounceMs": 120,
  "minQueryLength": 2,
  "items": []
}
```

| Field | Meaning |
|---|---|
| `id` | Stable provider id; namespaces item ids for frecency. Must equal `hello.name`. |
| `title` | Shown as the result badge and the scope chip. |
| `prefix` | Default scope keyword. Config may override it. Optional. |
| `dynamic` | `true` if the provider wants `query` notifications as the user types. |
| `debounceMs` | Minimum time between `query` notifications to this provider (default 0, max 1000). |
| `minQueryLength` | Don't send `query` for shorter text (default 0). |
| `items` | Static items, indexed and matched by Flint itself. May be empty. |

Result: `{}`. A provider can be static (only `items`), dynamic, or both
(Fenix commands are static; Jira search is dynamic; projects could be
both).

### `provider/setItems` (notification, provider → host)

`{ "items": [Item, ...] }` replaces the provider's static items, e.g. when
Fenix's open buffers change. Rate-limit on the provider side; Flint
re-indexes at most every 250 ms per provider.

### `query` (notification, host → provider)

```json
{
  "queryId": 731,
  "text": "ABC-12",
  "scoped": true,
  "context": {
    "foreground": { "process": "fenix.exe", "title": "main.rs — fenix", "pid": 7016 },
    "fenix": { "project": "C:\\dev\\fenix", "file": "C:\\dev\\fenix\\src\\main.rs" }
  }
}
```

- `queryId` increases monotonically per host.
- `scoped` is `true` when the user typed this provider's prefix; the text
  is then without the prefix.
- `context.fenix` is present only when Fenix is the foreground window and
  answered Flint's state request.

### `results` (notification, provider → host)

```json
{ "queryId": 731, "items": [Item, ...], "done": false }
```

Send any number of batches; the last has `done: true`. Batches of at most
200 items. Flint discards results whose `queryId` is not the latest, and
any arriving more than 2 s after the query was sent.

### `cancel` (notification, host → provider)

`{ "queryId": 731 }` — the user typed again. Stop work for that query if
you can; sending its results anyway is harmless.

### `execute` (request, host → provider)

Sent only for actions of kind `provider` (see Actions).

```json
{ "itemId": "jira:ABC-123", "actionId": "start-timer", "query": "ABC-12" }
```

Result:

```json
{ "close": true, "message": "Clocked in on ABC-123" }
```

`close: false` keeps Flint open (for actions that refine the list).
`message` is shown in the footer. On failure, return a JSON-RPC error;
its `message` is shown in the footer and Flint stays open. Timeout: 10 s,
after which Flint shows "jira didn't answer" and stays open.

### `preview` (request, host → provider) — reserved

`{ "itemId": "..." }` → `{ "markdown": "..." }`. Not implemented before
milestone 7; providers must not rely on it yet.

### `log` (notification, provider → host)

`{ "level": "info" | "warn" | "error", "message": "..." }` → Flint's log
for that provider.

## Item

```json
{
  "id": "jira:ABC-123",
  "title": "ABC-123 Telemetry export drops the last frame",
  "subtitle": "In Progress · assigned to you",
  "icon": "ticket",
  "keywords": ["telemetry", "export"],
  "actions": [Action, ...]
}
```

| Field | Meaning |
|---|---|
| `id` | **Stable** across sessions and unique within the provider. Frecency is keyed on it. Prefix with the provider id. Max 512 bytes. |
| `title` | Matched and highlighted. Max 300 characters. |
| `subtitle` | Shown dimmed; also matched, at a lower weight. Optional. |
| `icon` | A name from Flint's icon set (`app`, `file`, `folder`, `project`, `link`, `ticket`, `merge-request`, `command`, `terminal`, `window`, `calculator`, `note`, `task`) or an absolute path to an `.ico`/`.png`. Optional. |
| `keywords` | Extra words to match, not shown. Optional. |
| `actions` | At least one. The first is the `Enter` action. |

## Action

```json
{ "id": "open", "title": "Open in browser", "key": "o", "kind": "open", "target": "https://jira.example.com/browse/ABC-123" }
```

Declarative kinds are carried out by Flint itself, so static providers
need no round trip and Flint applies one policy to all of them:

| `kind` | Fields | Flint does |
|---|---|---|
| `open` | `target`: path or URL | opens with the OS default handler |
| `reveal` | `target`: path | shows it in the file manager |
| `copy` | `text` | puts it on the clipboard |
| `run` | `argv`: string array, `cwd`: optional path, `terminal`: bool | spawns the program directly (never through a shell); in the user's terminal if `terminal` |
| `fenix` | `method`, `params` | calls Fenix's control protocol (FENIX-INTEGRATION.md); disabled with a reason when Fenix lacks the capability |
| `query` | `text` | replaces the query line (drill-down) and keeps Flint open |
| `provider` | — | sends `execute` to the provider |

`key` is an optional one-character accelerator in the action menu.
Unknown kinds are shown disabled, not dropped, so a newer provider on an
older Flint fails visibly.

## Client messages

After `hello` with `role: "client"`:

| Method (request) | Params | Result |
|---|---|---|
| `show` | `{ "query"?: string, "activationToken"?: string }` | `{}` |
| `hide` | `{}` | `{}` |
| `toggle` | `{ "activationToken"?: string }` | `{ "visible": bool }` |
| `reload` | `{}` | `{ "errors": [string] }` (config problems, if any) |
| `status` | `{ "timings"?: bool }` | a Status, below |
| `quit` | `{}` | `{}`, then the daemon exits |

Params may be left out when every field is optional. A Status:

```json
{
  "version": "0.1.0",
  "pid": 9932,
  "uptimeMs": 125000,
  "coldStartMs": 649,
  "visible": false,
  "adapter": { "name": "NVIDIA GeForce GTX 1660 Ti", "backend": "Dx12", "fallback": false },
  "hotkey": { "chord": "Alt+Space", "state": "registered", "message": null },
  "providers": [ { "name": "jira", "state": "running", "message": null } ],
  "memoryBytes": 119600000,
  "settingsErrors": [ "`appearance.width`: 5000 is out of range (400–2000); using 720" ],
  "settingsFile": "C:\\Users\\me\\AppData\\Roaming\\flint\\settings.toml",
  "logFile": "C:\\Users\\me\\AppData\\Local\\flint\\logs\\flint.log",
  "theme": "Visual Studio Dark (following Fenix)",
  "font": "Consolas",
  "timings": {
    "samples": [ { "source": "hotkey", "ms": 15.1, "atUnixMs": 1791731045123 } ],
    "p50Ms": 15.1,
    "p95Ms": 15.1
  }
}
```

| Field | Meaning |
|---|---|
| `coldStartMs` | process start to ready (window, GPU and endpoint up); budget 1.5 s |
| `adapter` | `null` until the GPU is ready; `fallback` is the software adapter (WARP, llvmpipe) |
| `hotkey.state` | `registered`; `failed` (the OS refused it, `message` says why); `unsupported` (no global hotkeys here, as on Wayland) |
| `providers` | always empty before M5 |
| `memoryBytes` | resident memory where the OS reports it; budget 120 MB |
| `theme`, `font` | what the window is drawn with, and why (`following Fenix`, `set in settings.toml`) |
| `timings` | only when asked for: the last 100 shows, request to first presented frame, oldest first; budget 50 ms at p95 |

`activationToken` is the `XDG_ACTIVATION_TOKEN` from the client's
environment on Wayland; see DESIGN.md.

## Error codes

| Code | Name | When |
|---|---|---|
| -32700 … -32600 | JSON-RPC standard | malformed JSON, invalid request, unknown method, bad params |
| -32001 | UnsupportedProtocol | `hello.protocol` not supported |
| -32002 | NotReady | a message before the `hello` exchange completed |
| -32003 | NotDeclared | a connecting provider that config doesn't declare |
| -32004 | Unavailable | the action needs something absent (Fenix not running, capability missing); `message` says what |
| -32010 | ProviderFailed | the provider's own failure; `message` is shown to the user |

## Versioning

`protocol` is an integer that changes only for incompatible changes.
Adding optional fields, methods, icon names or action kinds is compatible
and does not bump it: both sides must ignore unknown fields, and unknown
methods are answered with "method not found" (requests) or ignored
(notifications).

## A complete exchange

```
provider → host  hello {protocol:1, role:"provider", name:"jira", version:"0.1.0", pid:4120}
host → provider  result {protocol:1, host:{name:"flint", version:"0.1.0", pid:9932}}
provider → host  provider/register {id:"jira", title:"Jira", prefix:"j", dynamic:true, debounceMs:120, minQueryLength:2, items:[]}
host → provider  result {}
                 … user types "j ABC" …
host → provider  query {queryId:12, text:"ABC", scoped:true, context:{…}}
provider → host  results {queryId:12, items:[…], done:true}
                 … user presses Tab, picks "Start timer" …
host → provider  execute {itemId:"jira:ABC-123", actionId:"start-timer", query:"ABC"}
provider → host  result {close:true, message:"Clocked in on ABC-123"}
```
