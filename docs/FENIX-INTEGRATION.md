# Flint and Fenix

What Flint uses from Fenix, how it degrades when Fenix can't give it
something, and the changes Fenix needs for the integration to be reliable.
Every statement about Fenix here was checked against its source at
`f05780740b7d45e41eedc35b846dba7c832f4d49` (2026-10-11), and the control
channel against `ce118a2af1a0b70733d0a2dd426f94365cf5eb1a` (2026-10-13,
Fenix #58 and #59), the revision Flint now pins; file references are
relative to the Fenix repository.

## Principles

1. **Fenix never depends on Flint.** Everything Fenix gains here is
   useful on its own (a better single-instance hand-off, a scriptable
   control channel, `fenix file:line`), and Fenix keeps working with
   Flint absent.
2. **Flint never writes Fenix's files.** It reads Fenix's state; anything
   that changes Fenix's data goes through Fenix itself. Fenix's stores
   are not built for a second writer (see F8).
3. **Capabilities, not versions.** Flint asks Fenix what it can do and
   enables or disables each action accordingly, with the reason shown.
4. **One framing, one envelope.** Fenix's control channel uses the same
   `Content-Length` + JSON-RPC 2.0 shape as LSP, DAP and Flint's provider
   protocol, built on `fenix-rpc`.

## What exists in Fenix today

| Thing | Where | Useful to Flint as is? |
|---|---|---|
| Control channel (since #58): a per-user named pipe / Unix socket, JSON-RPC with `fenix/hello` (capabilities, pid), `fenix/open` (line, column), `fenix/openProject`, `fenix/state`; `fenix file:12:3`, `--goto`, `--project`. Replaces the loopback TCP hand-off on port 47821. | `crates/fenix-control`, `docs/CONTROL.md` | Yes: Flint's level 1 (M7), through `fenix-control` itself. |
| Folder layout, `FENIX_HOME`, keyed JSON state files with atomic writes | `crates/fenix-storage/src/paths.rs`, `state.rs` | Yes, as a git dependency. |
| Known projects: `state/projects.json`, key `known` (array of paths, most recent first); pins and groups under key `meta` (`{ "pinned": [path], "groups": { path: name } }`) | `crates/fenix-project/src/known.rs`, `meta.rs` | Yes, read-only. |
| Recent files: `state/recent.json`, key `files` (array, most recent first, max 200); key `dirs` for the explorer's folders | `crates/fenix-project/src/recent.rs` | Yes, read-only. |
| Settings: `settings.toml` (`editor.theme`, `editor.font_family`, `jira.*`, `gitlab.*`) | `docs/CONFIGURATION.md` | Yes, read-only, for theme and font. |
| Command registry: 256 commands, each `id` + `description`, run by id; `description` is marked `dead_code`, "will back a command palette" | `crates/fenix-gui/src/commands.rs` | Not reachable from outside the process. |
| Leader trie: key sequences → command ids, with labels and groups | `crates/fenix-gui/src/keymap.rs` (`leader_trie`), `crates/fenix-keymap` | `fenix-keymap` yes (zero dependencies); Fenix's actual bindings no. |
| Themes: eight palettes as `Theme` constants built from `glyphon` types | `crates/fenix-gui/src/theme.rs` (`ALL`, `by_name`) | No; Flint has to copy colours. |
| Agenda: `data/agenda.json`, written whole by Fenix, read once at start, never re-read | `crates/fenix-agenda/src/persist.rs` | Read-only, and only as a snapshot. |
| Notebook: plain files under `data/notebook` with an index | `crates/fenix-notebook` | Read-only. |
| `Content-Length` framing | `crates/fenix-rpc` | Yes; `read_message_limited` (F16) bounds reading. |

## What Flint uses today

### Level 0: Fenix's files (milestone M3)

Implemented in M3 (`flint-fenix`), and still what Flint does with no
Fenix running. Flint:

- resolves Fenix's folders with `fenix_storage::paths::Roots::current()`
  (so a portable `FENIX_HOME` install works);
- reads `state/projects.json` (`known`, `meta`) and `state/recent.json`
  (`files`) with `fenix_storage::state::read`, re-reading when the file's
  modification time changes;
- strips Windows' `\\?\` prefix from paths before showing or opening them.
  Fenix does this for projects (`fenix_project::plain_path`) but stores
  recent files canonicalized (`App::record_recent_file` calls
  `std::fs::canonicalize`), so on Windows they come back as
  `\\?\C:\...`;
- treats a missing, empty or unreadable file as "no items", the same
  posture Fenix takes;
- with no Fenix answering on the control channel, starts one with the
  request as flags: `fenix --project <root>`, `fenix --goto
  <path:line[:col]>`, or `fenix -- <paths>` (`fenix.executable` when set,
  else `fenix` on `PATH`, else beside `flint`). The new process gets the
  foreground from Flint as it starts. (Before M7, Flint spawned `fenix
  <path>` for every open and lifted the foreground lock for everyone,
  `ASFW_ANY`, because it couldn't name Fenix's pid; that is gone.)
- follows `editor.theme` and `editor.font_family` from Fenix's
  `settings.toml`. Instead of a watcher thread, Flint checks the file's
  modification time before each show and re-reads it when it changed,
  which is when a difference could be seen; the state files are checked
  the same way after each show. Only those two keys are read: the file
  also holds Fenix's tokens, which Flint never reads (F12).

### Level 1: the control channel (milestone M7)

With a Fenix running, Flint talks to it over Fenix's control channel,
using Fenix's own `fenix-control` crate (its endpoint naming in
`flint-platform`, its client in `flint-fenix`), so the endpoint can't be
named differently from Fenix. On a worker thread, never the event loop:

- before each show, a *look*: `fenix/hello` says whether a Fenix answers,
  its pid and its `capabilities`. Each Fenix action in the menu is enabled
  by the method being listed there, never by version, and otherwise
  dimmed with the reason. `flint status` reports "level 1" and the list;
- when Fenix is the window Flint was summoned from, `fenix/state` too: the
  project and file go to providers as `context.fenix` (PROTOCOL.md);
- *Open in Fenix* on a project (now `Enter`) is `fenix/openProject`; on a
  recent file, `fenix/open`; and a typed absolute path, optionally
  `:line[:col]`, is a result that opens it there. Flint first grants
  Fenix's pid the foreground (F3);
- Fenix's answer settles it: Flint hides once Fenix has done it, or shows
  Fenix's reason in the footer and stays.

Since Fenix #59, and on the same look, Fenix's commands, notebook capture
and the agenda:

- `: ` lists the commands Fenix marks runnable from outside (`external`),
  by description, with the keys that run them in Fenix (`SPC a a`) in the
  subtitle; `Enter` is `fenix/runCommand`, and Fenix comes forward.
  `fenix:<command>` leader bindings use the same method;
- `t ` lists the agenda's open tasks (`fenix/agenda.tasks`), the one being
  clocked first (`fenix/state`'s `agendaTimer`); `Enter` clocks in, or out
  on the running one (`fenix/agenda.clockIn`, `clockOut`);
- `n <text>` captures into the notebook's Inbox, or today's journal from
  the menu (`fenix/notebook.capture`), without Fenix taking the focus.

Capture and the clock happen behind the scenes, so Flint stays open and
confirms in the footer; Fenix stays the only writer of its agenda and
notebook (F8). Each is offered only when the running Fenix lists the
method; with no Fenix running, or an older one, the prefix says why.

## Changes Fenix needs

Ordered so the first pull request unblocks the most. "Blocks" names the
Flint milestone that waits on it.

### F1. A control protocol <a id="f1"></a>

**Landed in Fenix #58** (`hello`, `open`, `openProject`, `state`) **and
#59** (`commands`, `runCommand`, `notebook.capture`, `agenda.tasks`,
`agenda.clockIn`, `agenda.clockOut`, and `agendaTimer` in `state`). Task
ids are JSON numbers; statuses are `todo`, `inProgress`, `blocked`,
`done`.

Today the hand-off carries a list of paths, one way, with no reply. Flint
needs to ask Fenix things and to tell it more than "open these files".

Replace the body of the single-instance channel with JSON-RPC 2.0 over
`fenix-rpc` framing, on the per-user endpoint from F2. Keep accepting the
legacy newline-separated payload for one release, detected by the first
bytes not being `Content-Length:`, so an older `fenix` binary on `PATH`
still hands off.

Methods (Fenix is the server):

| Method | Params | Result | Notes |
|---|---|---|---|
| `fenix/hello` | `{ protocol: 1, client: { name, version, pid } }` | `{ protocol: 1, version, pid, capabilities: [string] }` | `pid` is needed for F3. `capabilities` lists the method names below that this build implements. |
| `fenix/open` | `{ paths: [{ path, line?, column? }], newWindow?: bool, focus?: bool }` | `{}` | Subsumes the current hand-off and adds positions (F4). |
| `fenix/openProject` | `{ root }` | `{}` | Same as choosing it in `SPC p p`, including adding it to known projects. |
| `fenix/state` | `{}` | `{ project?: path, file?: path, line?: n, agendaTimer?: { taskId, title, since } }` | For Flint's context ranking. |
| `fenix/commands` | `{}` | `{ commands: [{ id, description, keys?: string, external: bool }] }` | F6. |
| `fenix/runCommand` | `{ id }` | `{}` or error | Only commands marked `external`. |
| `fenix/notebook.capture` | `{ text, target?: "inbox" \| "journal" }` | `{ entry: path }` | F5. |
| `fenix/agenda.tasks` | `{ status?: [string] }` | `{ tasks: [{ id, title, status, jiraKey?, elapsedMinutes }] }` | So Flint never parses `agenda.json` (F8). |
| `fenix/agenda.clockIn` | `{ taskId }` | `{}` | |
| `fenix/agenda.clockOut` | `{}` | `{}` | |

Notification (Fenix → client, optional for v1): `fenix/stateChanged`
with the `fenix/state` shape, for clients that keep a connection open.

Implementation sketch in Fenix: the accept loop in `ipc.rs` stays a
background thread; each request becomes a `FenixUserEvent` carrying a
oneshot reply sender, handled on the UI thread like every other
cross-thread event (`OpenFiles` today), so no new locking is introduced.

`fenix --goto file:line:col` and `fenix --project <dir>` CLI flags should
map onto the same methods, which makes the protocol testable from a
terminal and useful to other tools (a `git difftool`, a terminal's
hyperlink handler).

### F2. A per-user endpoint instead of a fixed TCP port <a id="f2"></a>

**Landed in Fenix #58**, with exactly the naming below, documented in
Fenix's `docs/CONTROL.md` and implemented once in `fenix-control`, which
Flint uses.

`127.0.0.1:47821` is one port for the whole machine. On a shared machine
(Windows Remote Desktop Services, a Citrix host, a shared Linux lab box,
a build VM people RDP into) the first user to start Fenix owns the port,
and every other user's `fenix somefile` **sends that user's file path to
the first user's editor**, which opens it there. It also lets any local
process, any user, make your Fenix open a path. And as `ipc.rs` already
notes, an unrelated program on that port makes Fenix silently run
standalone.

Use a Windows named pipe whose name includes the user's SID, created with
a DACL for that SID only and `FILE_FLAG_FIRST_PIPE_INSTANCE`; on Linux a
Unix socket in `$XDG_RUNTIME_DIR/fenix/` (directory `0700`). The
`interprocess` crate covers both. Flint uses the same scheme for its own
endpoint, so a shared helper is a candidate for a small crate both use.

### F3. Focus hand-off on Windows <a id="f3"></a>

**Landed in Fenix #58**: `fenix/hello` reports the pid, and Fenix's own
client grants it before handing off.

`apply_open_files` calls `window.focus_window()`. On Windows a
background process may not take the foreground; the call usually just
flashes the taskbar button. The process that *is* in the foreground (the
second `fenix.exe` Explorer just started, or Flint) has to permit it with
`AllowSetForegroundWindow(fenix_pid)` before sending the request.

- Fenix side: report `pid` in `fenix/hello` (F1); in its own client path
  (the second `fenix.exe`), call `AllowSetForegroundWindow(ASFW_ANY)` or
  the server's PID before handing off.
- Worth verifying on a real desktop first: double-click a file in Explorer
  while Fenix is behind another window. If Fenix doesn't come forward,
  this is already a Fenix bug independent of Flint.

### F4. Positions when opening <a id="f4"></a>

**Landed in Fenix #58** (`fenix/open` takes `line`/`column`;
`fenix file:12:3`, `--goto`, sharing `fenix-tasks`' parser).

Nothing in the open path accepts a line or column today
(`open_file_from_picker`, `open_startup_file`). Add `line`/`column` to
`fenix/open`, and `path:line[:column]` parsing on the command line,
careful with Windows drive letters (`C:\x.rs:12:3`). `fenix-tasks`
already parses `path:line:col` from compiler output
(`crates/fenix-tasks/src/output.rs`); share that parser rather than
writing a second one.

### F5. Commands that take arguments <a id="f5"></a>

**Landed in Fenix #59**, as control methods implemented against `App`
(capture and the clock each had one place doing them already).

`CommandFn` is `fn(&mut CommandCtx)` with no parameters, and commands
like `notebook.capture` and `agenda.toggle_clock` open a prompt or act on
the current selection inside Fenix. An external caller needs to pass the
text or the task id. Either give `Command` an optional handler taking
`serde_json::Value`, or implement the few parameterised control methods
in F1 directly against `App`. The second is smaller; the first scales.

### F6. Command metadata for outside callers <a id="f6"></a>

**Landed in Fenix #59**: `keys` from the leader trie (the shortest
binding), and 63 commands marked `external`, guarded by a test that runs
each with a text buffer in front and with a page in front.

The registry and the leader trie already hold everything needed: ids,
descriptions, and the key sequence bound to each id. Expose them through
`fenix/commands`, with:

- `keys`: the leader sequence as Fenix's which-key shows it (`SPC p p`),
  via `keymap::describe_keypress`;
- `external`: whether running it from outside makes sense. `file.save`,
  `edit.undo` or `git.hunk_stage` need a focused buffer and a user looking
  at it; `project.switch_project`, `agenda.open`, `notebook.journal` don't.
  Default `false`; mark the useful ones. A test should fail if an
  `external` command's handler assumes a focused text buffer.

### F7. A light crate for Fenix's state files <a id="f7"></a>

**Not blocking; removes a fragile coupling.**

Flint reads `projects.json` and `recent.json` by key name because
`fenix-project`, which owns them, depends on `fenix-mib` and
`fenix-ccsds`. Move `KnownProjects`, `ProjectMeta`, `RecentFiles` and
`plain_path` into a crate depending only on `fenix-storage` (or put the
MIB/CCSDS parts of `fenix-project` behind a default feature), and treat
the files' keys and shapes as a documented contract with a version field.
While there, store recent files in plain form like projects (see "What
Flint reads today").

### F8. Safe external changes to agenda and notebook <a id="f8"></a>

**Landed in Fenix #59**: capture and the clock go through Fenix, its
only writer, and Fenix reloads `agenda.json` when it changes on disk
(keeping what it has when the file can't be read).

`agenda.json` is loaded once and saved whole; a change made by anyone
else is overwritten at Fenix's next save. The notebook's index likewise
assumes Fenix made every change. F1's agenda and capture methods make
Fenix the only writer, which is the fix Flint needs. Separately, Fenix
reloading `agenda.json` when it changes on disk (it already does this for
`settings.toml`) would protect against the roaming-profile case: the file
lives under `%AppData%`, which roams, so two machines can write it.

### F9. Themes as plain data <a id="f9"></a>

**Not blocking; removes duplicated colours.**

Move the palettes out of `fenix-gui` into a `fenix-theme` crate with
plain `[f32; 4]` / `[u8; 4]` colours and the `name`/`ALL`/`by_name` API,
and have `fenix-gui` convert to `glyphon::Color` at the edge. Flint then
depends on it and drops its copy. Every future suite tool gets the themes
for free.

### F10. Owned labels in `fenix-keymap` <a id="f10"></a>

**Not blocking; Flint interns labels meanwhile** (`flint-gui`'s
`leader.rs`, since M4: the labels live in an arena owned beside the trie
and replaced with it on reload).

`KeyTrie::insert` and `label_group` take `&'static str` labels, and
`Hint`/`Matcher::label` return them. Fenix's bindings are compiled in, so
that works there; Flint's leader comes from config. Make labels
`Cow<'static, str>` (or generic), keeping Fenix's call sites unchanged.

### F11. Release tags <a id="f11"></a>

**Not blocking.** The repository has no tags, so Flint pins Fenix crates
by commit hash. Tagging releases (`v1.0.0`, ...) lets `Cargo.toml` say
which Fenix it was built against in a way a human can read.

### F12. One credential store for the suite <a id="f12"></a>

**Flint's half is done (M8):** it keeps tokens in the OS credential store
under exactly these names (`flint credential set`). When Fenix moves its
tokens there under the same names, both share them; until then each asks
for its own.

Fenix keeps Jira/GitLab/GitHub tokens in `settings.toml` (moving them to
the OS credential store is already planned in its docs). When it moves,
agree on entry names so every suite tool finds the same token: service
`fenix-suite`, account `jira:<base_url>`, `gitlab:<base_url>`,
`github:<host>`. Flint never reads tokens out of Fenix's `settings.toml`.

### F13. Fenix as a live provider (later) <a id="f13"></a>

**After M7; optional.**

With F1 Flint pulls from Fenix. Later, Fenix can also connect to Flint's
endpoint as a provider (`connect = true`, PROTOCOL.md) and push dynamic
items: open buffers, the current project's files and symbols, recent
notebook entries, open review threads. Fenix should do this only when
Flint's endpoint exists, silently skipping otherwise (principle 1).

### F14. App-agnostic folder resolution <a id="f14"></a>

**Not blocking; Flint mirrors the rules meanwhile.**

`fenix_storage::paths::Roots::platform` hard-codes `fenix` and
`FENIX_HOME`. A `Roots::for_app("flint", "FLINT_HOME")` constructor would
give every suite tool the same layout with no copied rules.

### F15. GPU adapter fallback <a id="f15"></a>

**Not needed by Flint; a Fenix robustness fix found while checking.**

`GpuContext::new` requests a high-performance adapter and `expect`s one,
so Fenix panics where no hardware adapter is usable (some VMs, some RDP
sessions). Retry with `force_fallback_adapter: true` before failing.

### F16. A bounded `fenix-rpc` reader <a id="f16"></a>

**Landed in Fenix #58** as `read_message_limited`. `flint-protocol` still
uses its own bounded reader; switching is a cleanup.

`read_message` allocates `vec![0u8; content_length]` from an unchecked
header and reads header lines with an unbounded `read_line`. A peer can
make the reader allocate gigabytes or read forever. Add
`read_message_limited(reader, max_body, max_header_line)` and use it for
anything that isn't a child process Fenix spawned.

## Summary

| # | Change | Blocks | Size |
|---|---|---|---|
| F1 | Control protocol (JSON-RPC on the single-instance channel) | landed (#58, #59) | medium |
| F2 | Per-user named pipe / Unix socket | landed (#58) | small |
| F3 | Foreground hand-off on Windows | landed (#58) | small |
| F4 | `path:line:col` on open | landed (#58) | small |
| F5 | Commands with arguments | landed (#59) | small–medium |
| F6 | Command metadata and `external` flag | landed (#59) | small |
| F7 | Light state crate; recent files in plain form | — | small |
| F8 | Single writer for agenda/notebook; reload agenda on change | landed (#59) | small |
| F9 | `fenix-theme` crate | — | small |
| F10 | Owned labels in `fenix-keymap` | — | small |
| F11 | Release tags | — | trivial |
| F12 | Shared credential naming | Flint uses the names (M8); Fenix still keeps tokens in `settings.toml` | small |
| F13 | Fenix pushes items to Flint | after M7 | medium |
| F14 | App-agnostic folder roots | — | small |
| F15 | GPU adapter fallback | — | small |
| F16 | Bounded `fenix-rpc` reader | landed (#58) | small |

The first Fenix pull request, **F2 + F16 + F1 (`hello`, `open`,
`openProject`, `state`) + F3 + F4**, landed as Fenix #58 and is what M7
builds on. The second, **F5 + F6 + the rest of F1 + F8**, landed as
Fenix #59, completing M7. What remains (F7, F9–F11, F13–F15, and F12's
Fenix half) blocks nothing in Flint.
