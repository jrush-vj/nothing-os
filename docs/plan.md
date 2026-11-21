# nothing-bar: build plan

Crate at `bar/` (Rust, edition 2021). Deps: smithay-client-toolkit 0.21 (layer shell, shm, pointer),
tiny-skia (drawing), ab_glyph (JetBrains Mono from the system, OFL), libc (localtime, signals). Data comes from
/proc, /sys, Hyprland's sockets and the CLI tools already installed (wpctl, playerctl, nmcli, makoctl, ...).
Settings: `~/.config/nothing-bar/config` (plain `key = value`), re-read when it changes.

| # | Slice | Done when |
|---|---|---|
| 1 | Skeleton: layer surface, shm + tiny-skia, text, frame-driven animation core, settings file; clock, workspaces (click, scroll, pop, grow), app · title, CPU/RAM; replaces Waybar in autostart | bar runs on the LOQ, idle CPU ~0, unit tests pass |
| 2 | Status cluster: network rate, brightness, volume (scroll/middle/right), battery + sweep, DGPU/TURBO, hover pill glide, tooltips, btop click; urgent breathe, clock roll, count tween, login cascade | all items live and animated |
| 3 | Media: playerctl, slide-in controls, progress line | plays/pauses from the bar |
| 4 | Control centre panel: toggles, power profile, RTX status, sliders, media card, graphs, session (click twice), settings page | every control works |
| 5 | Notifications: bell + panel (makoctl), mako restyle; tray popover (StatusNotifierItem) | |
| 6 | Remaining settings: shapes, heights, positions, materials, workspace styles, clock formats | each toggle applies live |

## Round 2: motion and widgets (2026-11-21)

Motion and widgets in the Nothing language (dot matrix, one
red signal, no bounce): no curtains, no Quickshell/Qt. Everything stays in nothing-bar
or the Lua config.

| # | Slice | Idea → nothing-os version | Done when |
|---|---|---|---|
| 7a | Motion | full-page workspace slide → `slidefade 18%`; red curtain → dot-matrix screen that shrinks away from the bar's corner; scramble roll → labels decode left to right with a red write head | runs on the LOQ, turbo skips it |
| 7b | Lock | Phase line lock → nothing-bar session lock (ext-session-lock): Doto clock, a dot field that ripples on each key, red dot per typed char, PAM; hyprlock stays the fallback | locks/unlocks on the LOQ |
| 7c | Player | glyph-matrix art → album art as a dot grid sized by brightness (Glyph style), hover shows colour; seek line | plays/pauses/seeks; needs an image decoder (ask) |
| 7d | Notes | Quick Notes drawer → `SUPER+N` drawer from the bar, plain text in `~/.local/share/nothing-bar/notes`, autosave | survives restart |
| 7e | Clipboard | grid history → `SUPER+V` panel over cliphist, text rows + image dots, pin, delete | replaces the fuzzel picker |
| 7f | Wallpaper | Phase picker → dot-grid tile/colour picker with a dot-dissolve between wallpapers (swaybg) | applies and persists |
