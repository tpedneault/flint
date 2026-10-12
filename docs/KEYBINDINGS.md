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
| `Tab` | opens the action menu |
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

| `Tab`, `.` | opens the action menu |
| `SPC` | opens the leader |

### The action menu

`Tab` (in either mode) or `.` (in Normal mode) lists what the selected
result can do. The first entry is what `Enter` does.

| Key | Does |
|---|---|
| `j`, `k`, `Down`, `Up`, `Ctrl-n`, `Ctrl-p` | moves |
| `Enter` | runs the selected action |
| the action's key | runs that action |
| `Esc`, `Tab` | closes the menu |

Actions use the same keys everywhere: `o` opens, `l` launches, `e` opens
in Fenix, `f` shows in its folder, `t` opens a terminal there, `c` copies.
An action Flint can't run yet (opening a project in Fenix, which needs a
newer Fenix) is dimmed, says why, and stays put if chosen.

### The leader

`SPC` in Normal mode lists the leader's keys and what each does. Press a
key to run it, or to open its group (`f`, then `r`); `Backspace` goes
back a key and `Esc` cancels. The keys are yours to set in `[leader]`
([CONFIGURATION.md](CONFIGURATION.md#the-leader)); out of the box, `SPC p`
lists Fenix projects, `SPC f` recent files, `SPC a` apps, `SPC l` Flint's
commands, `SPC r` reloads settings and `SPC s` opens them.

## What `Enter` does

| Result | `Enter` |
|---|---|
| an app | launches it, as the Start menu would (Windows) or as its `.desktop` file says (Linux) |
| a Fenix project | opens its folder (opening it *in Fenix* needs a newer Fenix; it says so) |
| a Fenix recent file | opens it in Fenix, handing it to the Fenix already running |
| a quicklink taking a query | puts its keyword in the query line, so you can type what fills it |
| a scoped quicklink (`j ABC-123`) | opens the filled-in link |
| a sum (`2 * 21`, `= sqrt(2)`) | copies the result |
| a shell command (`> cargo build`) | runs it in a new terminal window; `Tab` `h` runs it hidden and reports how it ended |
| a window (`w `) | brings it to the front, restoring it if minimised; `Tab` `x` asks it to close |
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
| `w` | open windows (Windows), the one you were in first; Linux lists none yet and says why |
| `=` | the calculator: `+ - * / %`, `^`, parentheses, `0x`/`0b` numbers, `sqrt`, `sin`, `pi`… A sum typed without `=` is answered too. |
| `>` | the shell: what you type is run as a command (see CONFIGURATION.md, "The shell prefix") |
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
