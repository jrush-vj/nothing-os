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
