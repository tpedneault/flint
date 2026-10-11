# Flint on KDE Plasma 6 (Wayland)

Wayland gives ordinary applications no global hotkeys and doesn't let
them place their own windows. Flint works within that: KDE provides the
shortcut and the placement, and Flint does the rest. Three steps, once.

## 1. Start Flint at login

Add an autostart entry that runs `flint`: **System Settings → Autostart →
Add… → Add Application…**, and choose the `flint` binary. With no command,
`flint` starts in the background and waits; it shows nothing until asked.

(`flint autostart on` arrives in M6.)

## 2. Bind a shortcut to `flint toggle`

**System Settings → Keyboard → Shortcuts → Add New → Command or Script…**

- Command: `flint toggle`
- Shortcut: whatever you like; `Meta+Space` or `Alt+Space`. KDE uses
  `Alt+Space` for KRunner by default; unbind it there first if you want
  that chord for Flint.

`flint toggle` starts Flint if it isn't running yet, so the shortcut works
even without step 1; the first press then takes a moment longer.

**Focus.** KWin's focus-stealing prevention keeps a window behind others
unless the request to show it came from what you just did. KDE gives the
command it runs for a shortcut an activation token in
`XDG_ACTIVATION_TOKEN`; `flint toggle` passes it to the running Flint,
which presents it when its window opens, so KWin lets Flint come to the
front. If Flint appears without focus, that hand-off failed: please report
it with your Plasma version.

**Why the window is made on each show.** On Wayland a window can't be
hidden, only destroyed, so there Flint opens its window when shown and
closes it when hidden. Its GPU device and fonts stay loaded, so showing is
still quick, but it is not quite the pre-created window Windows gets.

## 3. Install the window rule

Wayland windows can't position themselves. The rule in
[`contrib/kde/flint.kwinrule`](../contrib/kde/flint.kwinrule) asks KWin to
centre Flint, keep it above other windows, and leave it out of the task
bar and pager:

**System Settings → Window Management → Window Rules → Import…**, and
choose the file. It matches the window class `flint`, which Flint sets as
its Wayland app id (and its X11 `WM_CLASS`).

## X11

On an X11 session Flint registers its hotkey (`hotkey.toggle`) itself, and
places its window itself, so neither the shortcut nor the rule is needed.
X11 is supported on a best-effort basis.

## Checking

`flint status` says whether the hotkey is registered (on Wayland it reads
`none here`, which is expected), which GPU adapter is in use, and where
the log is.
