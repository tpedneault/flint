# Configuration

Flint runs without any settings file. To change something, create
`settings.toml` in Flint's settings folder and set only what you want
different; everything else keeps its default.

## Where things live

| What | Windows | Linux |
|---|---|---|
| Your settings, `settings.toml` | `%AppData%\flint` | `~/.config/flint` |
| The log, `flint.log` (and `flint.log.old`) | `%LocalAppData%\flint\logs` | `~/.local/state/flint/logs` |
| This machine's state: `frecency.json`, what you pick for what you type | `%LocalAppData%\flint\state` | `~/.local/state/flint/state` |

What you chose roams with your Windows profile; what Flint noticed about
this machine doesn't. The layout is Fenix's, under `flint` instead of
`fenix`.

`FLINT_HOME=<folder>` puts all of it in one folder instead: a portable
install, or a test run that mustn't touch your real settings. A Flint
with its own `FLINT_HOME` also has its own control endpoint, so it runs
beside your usual one rather than handing its commands to it.

`flint status` prints the settings file and log file the running Flint
uses.

## settings.toml

Hand-edit it. Flint never changes it; the only file it creates there is
an empty `settings.toml` (with a comment pointing here) when you choose
*Open settings* and there isn't one yet. Changes are picked up the next time
Flint is shown, or at once with `flint reload`, which also prints any
problems:

```
> flint reload
Settings reloaded with 1 problem(s):
  `appearance.width`: 5000 is out of range (400–2000); using 720
```

A bad value costs only that setting: Flint uses its default, says so in
the footer, in `flint reload` and in `flint status`, and keeps running.
A key Flint doesn't know is reported the same way, so a typo doesn't go
unnoticed. A file that isn't valid TOML at all is reported with its line
number, and every setting keeps its default until it's fixed.

```toml
[hotkey]
toggle = "Ctrl+Shift+Space"

[appearance]
width = 800
rows = 7

[appearance]
theme = "fenix"            # or "Nord", "Dracula", ...

[behaviour]
hide_on_blur = true
escape_hides = false

[fenix]
executable = 'C:\tools\fenix\fenix.exe'

[leader]
"p"   = "scope:fenix.projects"
"f r" = "scope:fenix.recent"
"f"   = { label = "Fenix" }
"j"   = "scope:quicklink.j"
"r"   = "flint:reload"

[[quicklinks]]
keyword = "j"
name = "Jira issue"
url = "https://jira.example.com/browse/{query}"

[[quicklinks]]
keyword = "mr"
name = "GitLab merge requests"
url = "https://gitlab.example.com/dashboard/merge_requests?search={query}"

[[quicklinks]]
keyword = "w"
name = "Outlook"
url = "https://outlook.office.com"
```

### Every setting

This table is generated from Flint's settings schema; a test fails when
it is out of date.

| Setting | Takes | Default | What it does |
|---|---|---|---|
| `hotkey.toggle` | a key chord, like `"Ctrl+Shift+Space"` | `"Alt+Space"` | The global hotkey that shows and hides Flint (Windows, and X11 best effort). On Wayland there are no global hotkeys: bind `flint toggle` in the desktop's shortcut settings instead (docs/KDE.md). `Alt+Space` shadows Windows' own window-menu shortcut while Flint runs. |
| `appearance.width` | a whole number, 400–2000 | `720` | The window's width, in logical pixels (scaled with the display). |
| `appearance.rows` | a whole number, 1–30 | `9` | How many results are shown at once; the window's height follows. |
| `appearance.theme` | one of `"fenix"`, `"Orbit Dark"`, `"TempleOS"`, `"Gruvbox Dark"`, `"Nord"`, `"Dracula"`, `"Solarized Dark"`, `"One Dark"`, `"Visual Studio Dark"` | `"fenix"` | The colours. `fenix` follows Fenix's `editor.theme` (and `editor.font_family`), changing when Fenix's does; a theme's name pins it. Orbit Dark when Fenix isn't installed or names a theme Flint doesn't have. |
| `behaviour.hide_on_blur` | `true` or `false` | `true` | Hide Flint when it loses focus, as when you click another window. |
| `behaviour.escape_hides` | `true` or `false` | `false` | Make `Esc` hide Flint from either mode, as Raycast does. By default `Esc` in Insert mode goes to Normal mode, where a second `Esc` (or `q`) hides. |
| `fenix.executable` | text | `""` | The `fenix` program Flint starts when no Fenix is running to ask (a running one is reached over its control channel). Empty: `fenix` on the `PATH`, else beside the `flint` program. |
| `shell.program` | text | `""` | The shell that runs what you type after `> ` (`pwsh`, `cmd`, `bash`, a full path). Empty: `pwsh` if it is on the `PATH`, else Windows PowerShell; on Linux, `$SHELL`, else `/bin/sh`. |
| `leader` | a table of key sequences and actions | `see below` | What `SPC` sequences do in Normal mode: `"f r" = "scope:fenix.recent"`. An action is `scope:<provider>`, `flint:<command>`, `fenix:<command>` or `query:<text>`; a table `{ action = ..., label = ... }` adds a label, and `{ label = ... }` names a group. Without the table, the defaults below apply; with it, the table is the whole map. |
| `providers` | entries with `name`, `command`, `connect`, `prefix`, `credential` | `[]` | External providers (docs/PROVIDERS.md). `command` is the program and its arguments, as a list, started by Flint in the settings folder and talked to over its stdin and stdout; `connect = true` instead waits for a running program to connect to Flint itself. `prefix` overrides the provider's own scope keyword. `credential` names a secret in the OS credential store (`jira:https://jira.example.com`, stored with `flint credential set`) that Flint gives the provider when it starts it. Only providers named here ever run. |
| `quicklinks` | entries with `keyword`, `name`, `url` | `[]` | Links with a keyword: typing the keyword and a space scopes the query to the link, and `Enter` opens `url` with `{query}` replaced by what you typed, percent-encoded. A link without `{query}` just opens. Written as `[[quicklinks]]` tables (see the example above). |

### Following Fenix

With `appearance.theme = "fenix"` (the default) Flint draws in the theme
Fenix uses (`editor.theme` in Fenix's `settings.toml`) and in Fenix's font
(`editor.font_family`, else the theme's own, as Fenix does). When you
change either in Fenix, Flint picks it up the next time it is shown. Name
a theme instead (`"Nord"`) to pin it; `flint status` says which theme is
in use and why. A font that isn't installed falls back to the system's
monospace font.

Flint reads only those two keys from Fenix's settings, and never writes
Fenix's files. Fenix's folders are found the way Fenix finds them,
`FENIX_HOME` included.

Flint asks the Fenix that is running to open files and projects, over
the private channel Fenix keeps for that (Fenix's `docs/CONTROL.md`), and
learns there what Fenix can do; an action that Fenix doesn't offer is
dimmed with the reason. With no Fenix running, Flint starts one with the
request on its command line (`fenix --project <folder>`, `fenix --goto
<file:line:col>`). For that it looks for `fenix` on the `PATH`, then
beside the `flint` program; if Fenix is installed somewhere else (a
development build, a folder that isn't on the `PATH`), set
`fenix.executable`. `flint status` says which Fenix answers ("level 1")
or which `fenix` it would start ("level 0"). A portable Fenix
(`FENIX_HOME`) is reached only by a Flint started with the same
`FENIX_HOME`.

### Quicklinks

Each `[[quicklinks]]` table is a link Flint can open. Type its keyword and
a space and the query line shows the link's name as a chip; what you type
next fills `{query}` in the URL, percent-encoded (`j ABC-123` opens
`https://jira.example.com/browse/ABC-123`). Without a keyword the link is
still found by its name, and choosing it puts its keyword in the query
line for you. A URL without `{query}` simply opens.

- `keyword` must be one word, unique among your links (and is what a
  leader binding names: `scope:quicklink.j`). `a`, `p`, `f`, `w`, `=`,
  `>`, `:`, `t`, `n` and `flint` belong to Flint's Apps, Fenix projects,
  Fenix recent files, windows, calculator, shell, Fenix commands, the
  agenda, notebook capture and Flint's commands; a link that names one
  keeps working by name, and `flint status` says why its keyword doesn't.
- `url` needs a scheme (`https:`, `mailto:`, …).
- A bad entry is skipped with its reason; the others still work.

### Providers

Each `[[providers]]` table runs an external provider: a program that
adds results to Flint over the protocol in docs/PROTOCOL.md. Only the
providers named here ever run; docs/PROVIDERS.md says how to write one.

```toml
[[providers]]
name = "bookmarks"
command = ["python", "examples/provider-python/bookmarks.py"]

[[providers]]
name = "jira"
command = ["flint-jira", "--url", "https://jira.example.com"]
credential = "jira:https://jira.example.com"   # its token, from the OS credential store
prefix = "ji"         # instead of the provider's own scope keyword

[[providers]]
name = "notes"
connect = true        # a program already running connects to Flint
```

- `name` is lowercase letters, digits, `.`, `_` and `-`, unique, and must
  equal the name the provider gives in its `hello`.
- `command` is a list, the program then its arguments, never one string:
  Flint doesn't run a shell. It starts in the settings folder, so a
  relative path is relative to `settings.toml`.
- Give `command` or `connect = true`, not both.
- A bare program name (`flint-jira`) is found on the `PATH`, or else
  beside `flint` itself, where Flint's own providers ship.
- `credential` names a secret in the OS credential store (Windows
  Credential Manager, the Secret Service on Linux), shaped
  `<kind>:<where>`. Store it once with `flint credential set <name>`,
  which asks for it without echoing it; Flint gives it to the provider
  when it starts it. Without it stored, the provider isn't started and
  the footer says which command to run. Tokens never go in
  `settings.toml`.
- A bad entry is skipped with its reason; the others still run.
- `flint status` lists each provider and its state (`running`,
  `restarting`, `disabled` and why); what it writes to stderr goes to
  `logs/providers/<name>.log` beside `flint.log`.

### The shell prefix

`> ` runs what you type after it with your shell: `Enter` in a new
terminal window that stays open afterwards, `Tab` then `h` hidden, with
its exit status and the last line it printed in the footer when it ends.
Commands run in your home folder. `shell.program` picks the shell; the
command reaches it as one argument (`pwsh -Command <what you typed>`,
`bash -c <what you typed>`), so it means exactly what it would at that
shell's prompt. With `cmd`, a command containing double quotes may be
split differently than at the prompt, since `cmd` doesn't follow the
usual quoting rules; PowerShell, the default, does.

### The leader

`SPC` in Normal mode opens Flint's leader: a list of keys and what each
does, the way Fenix's which-key menu works. `[leader]` maps key sequences
(single characters separated by spaces, as the menu shows them: `f r`)
to actions:

| Action | Does |
|---|---|
| `scope:<provider>` | limits the query to `apps`, `flint`, `fenix.projects`, `fenix.recent`, `fenix.commands`, `fenix.agenda`, `fenix.capture`, `windows`, `calculator`, `shell`, an external provider's name or `quicklink.<keyword>`, and goes to Insert mode |
| `flint:<command>` | runs `reload`, `settings`, `log` or `quit` |
| `fenix:<command>` | runs a Fenix command by its id (`fenix:agenda.toggle_clock`), one Fenix lets other programs run (`: ` lists them); it says why when Fenix isn't running or won't |
| `query:<text>` | replaces the query line with `text` |

A table adds a label (`{ action = "flint:reload", label = "Reload" }`);
without one, the menu says what the action does (the scope's name, the
command's title). `{ label = "Fenix" }` names a group: the `f` that leads
to `f r`. A key either runs something or starts a longer sequence, never
both.

Without a `[leader]` table these apply:

| Keys | Does |
|---|---|
| `a` | Apps |
| `p` | Fenix projects |
| `f` | Fenix recent files |
| `w` | Open windows |
| `l` | Flint's commands |
| `r` | Reload settings |
| `s` | Open settings |

With a table, the table is the whole map, so leaving a default out
removes it.

### Key chords

A chord is modifiers and one key joined by `+`, in any order and any
case: `Alt+Space`, `ctrl+shift+k`, `Super+F12`, `F9`.

- Modifiers: `Ctrl` (or `Control`), `Alt`, `Shift`, `Super` (or `Win`,
  `Meta`).
- Keys: `A`–`Z`, `0`–`9`, `F1`–`F24`, `Space`, `Tab`, `Enter`, `Escape`,
  `Backspace`, `Insert`, `Delete`, `Home`, `End`, `PageUp`, `PageDown`,
  `Up`, `Down`, `Left`, `Right`, and `` ` - = [ ] \ ; ' , . / `` by their
  position on a US keyboard.

If another application already owns the chord, Flint keeps running
without a hotkey, says so in the footer and in `flint status`, and tries
again on the next `flint reload`. `flint toggle` works regardless, so a
shortcut bound to it in your desktop or keyboard tool is always a way in.

**About `Alt+Space` on Windows.** It is also Windows' shortcut for a
window's system menu (Restore, Move, Size, Close). While Flint runs, Flint
gets it instead. If you rely on that menu, pick another chord.
