# Keybindings

## Summoning Flint

| Where | How |
|---|---|
| Windows | the hotkey, `Alt+Space` by default (`hotkey.toggle`, see [CONFIGURATION.md](CONFIGURATION.md)) |
| X11 | the same hotkey, best effort |
| KDE Plasma on Wayland | a shortcut you bind to `flint toggle` ([KDE.md](KDE.md)) |
| Anywhere | `flint toggle`, `flint show`, `flint show --query "text"` from a terminal, a script or a desktop shortcut |

The hotkey shows Flint, or hides it if it is showing. Clicking another
window hides it too (`behaviour.hide_on_blur`).

## In the window

Flint has one mode for now, with Insert mode's keys. The Insert/Normal
split, `SPC` leader and which-key popup described in
[DESIGN.md](DESIGN.md) arrive in M2 and M4; `Esc` will then go to Normal
mode instead of hiding.

| Key | Does |
|---|---|
| typing | edits the query |
| `Backspace` | deletes the character before the caret |
| `Ctrl-w`, `Ctrl-Backspace` | deletes the word before the caret |
| `Ctrl-u` | clears the query |
| `Down`, `Ctrl-n` | selects the next row |
| `Up`, `Ctrl-p` | selects the previous row |
| `Enter` | nothing yet: actions arrive in M2 (the footer says so) |
| `Esc` | hides Flint |

`AltGr` combinations type their character (`AltGr-q` is `@` on a German
layout), even though Windows reports them as `Ctrl+Alt`.
