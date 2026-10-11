# Keybindings

## Summoning Flint

| Where | How |
|---|---|
| Windows | the hotkey, `Alt+Space` by default (`hotkey.toggle`, see [CONFIGURATION.md](CONFIGURATION.md)) |
| X11 | the same hotkey, best effort |
| KDE Plasma on Wayland | a shortcut you bind to `flint toggle` ([KDE.md](KDE.md)) |
| Anywhere | `flint toggle`, `flint show`, `flint show --query "text"` from a terminal, a script or a desktop shortcut |

The hotkey shows Flint, or hides it if it is showing. Clicking another
window hides it too (`behaviour.hide_on_blur`). Flint always opens in
Insert mode with an empty query.

## Modes

Flint uses Fenix's grammar: Insert mode for typing, Normal mode for
moving and acting. The badge at the right of the query line says which
mode you're in, and the caret is a bar in Insert mode and a block in
Normal mode.

### Insert mode

| Key | Does |
|---|---|
| typing | edits the query; results update as you type |
| `Backspace` | deletes the character before the caret; on an empty query, removes the scope chip |
| `Ctrl-w`, `Ctrl-Backspace` | deletes the word before the caret |
| `Ctrl-u` | clears the query |
| `Down`, `Ctrl-n` | selects the next result |
| `Up`, `Ctrl-p` | selects the previous result |
| `Enter` | runs the selected result's first action |
| `Esc` | Normal mode (or hides, with `behaviour.escape_hides = true`) |

`AltGr` combinations type their character (`AltGr-q` is `@` on a German
layout), even though Windows reports them as `Ctrl+Alt`.

### Normal mode

| Key | Does |
|---|---|
| `j`, `Down`, `Ctrl-n` | selects the next result |
| `k`, `Up`, `Ctrl-p` | selects the previous result |
| `gg` | selects the first result |
| `G` | selects the last result |
| `i`, `a`, `/` | back to Insert mode |
| `Enter` | runs the selected result's first action |
| `y` | copies the selected result's text (its path, URL or title) |
| `Esc`, `q` | hides Flint |

`Tab` and `.` (the action menu) and `SPC` (the leader, with which-key)
arrive in M4; until then they say so in the footer.

## What `Enter` does

| Result | `Enter` |
|---|---|
| an app | launches it, as the Start menu would (Windows) or as its `.desktop` file says (Linux) |
| a Fenix project | opens its folder (opening it *in Fenix* needs a newer Fenix; it says so) |
| a Fenix recent file | opens it in Fenix, handing it to the Fenix already running |
| a quicklink taking a query | puts its keyword in the query line, so you can type what fills it |
| a scoped quicklink (`j ABC-123`) | opens the filled-in link |
| Reload settings | reads `settings.toml` again and says what's wrong, if anything |
| Open settings | opens `settings.toml` in your editor, creating an empty one if needed |
| Show log | opens `flint.log` |
| Quit Flint | stops the background process |

Flint hides after handing off to something else, and stays open with the
reason in the footer when that fails.

## Scopes

A keyword followed by a space limits the query to one source; the
keyword becomes a chip at the start of the query line, and `Backspace`
on an empty query removes it.

| Keyword | Searches |
|---|---|
| `a` | installed apps |
| `p` | Fenix's projects, pinned first |
| `f` | Fenix's recent files, most recent first |
| `flint` | Flint's own commands |
| a quicklink's keyword | that link, filled with what you type |

## Ranking

Results are ranked by how well they match (fuzzily on the name, by
whole words on keywords and descriptions) and by what you have picked
before for queries starting with the same two letters, recent picks
counting more: a pick's weight halves every week. Choose Firefox after
typing `fi` a few times and it comes first for `fi`, without crowding
results for anything else. Results that arrive while the list is on
screen (a fresh apps scan, say) appear below the selected row, so
nothing jumps under your finger.
