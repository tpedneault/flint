# Configuration

Flint runs without any settings file. To change something, create
`settings.toml` in Flint's settings folder and set only what you want
different; everything else keeps its default.

## Where things live

| What | Windows | Linux |
|---|---|---|
| Your settings, `settings.toml` | `%AppData%\flint` | `~/.config/flint` |
| The log, `flint.log` (and `flint.log.old`) | `%LocalAppData%\flint\logs` | `~/.local/state/flint/logs` |
| This machine's state (from M2: frecency) | `%LocalAppData%\flint\state` | `~/.local/state/flint/state` |

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

Hand-edit it; Flint never writes it. Changes are picked up the next time
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

[behaviour]
hide_on_blur = true
```

### Every setting

This table is generated from Flint's settings schema; a test fails when
it is out of date.

| Setting | Takes | Default | What it does |
|---|---|---|---|
| `hotkey.toggle` | a key chord, like `"Ctrl+Shift+Space"` | `"Alt+Space"` | The global hotkey that shows and hides Flint (Windows, and X11 best effort). On Wayland there are no global hotkeys: bind `flint toggle` in the desktop's shortcut settings instead (docs/KDE.md). `Alt+Space` shadows Windows' own window-menu shortcut while Flint runs. |
| `appearance.width` | a whole number, 400–2000 | `720` | The window's width, in logical pixels (scaled with the display). |
| `appearance.rows` | a whole number, 1–30 | `9` | How many results are shown at once; the window's height follows. |
| `behaviour.hide_on_blur` | `true` or `false` | `true` | Hide Flint when it loses focus, as when you click another window. |

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

## Settings that arrive later

The design names more settings (`behaviour.escape_hides`,
`appearance.theme`, `[fenix]`, `[[quicklinks]]`, `[leader]`,
`[[providers]]`). Each joins this page with the milestone that makes it
do something; until then Flint reports it as unknown rather than silently
accepting a key that does nothing.
