# nothing-bar: design spec

Decided over four Bar Lab rounds (2026-11-10). "Setting" = switchable live from the bar settings page / config file;
the value given is the default.

## Engine
- Custom Rust bar: Wayland layer-shell, tiny-skia drawing into shm buffers, Hyprland IPC directly. No toolkit.
- Draws only while something moves; idle = no frames, ~0% CPU. Animations at the display's refresh (144 Hz);
  turbo (SUPER+F12) turns bar animations off.

## Look
- Font: JetBrains Mono for everything, 13.8 px regular (12 px + 15%, 2026-11-10). Labels grey (#999), values white.
- Shape (setting): **full-width strip** | floating | three islands | single centre island.
- Size (setting): 100 | 115 | **130** | 145% of a 28 px bar with 12 px text; scales the panel and tooltips too. Position (setting): **top** | bottom | auto-hide | shrink when busy.
- Bar material (setting): **solid #1a1a1a** | translucent | frosted (Hyprland blur, medium) | transparent | gradient fade | light.
- Gradient hairline under the bar (1 px, fades out at both ends). Full outline when the shape is floating.
- Panels: frosted glass, 22 px radius, shadow under panels only (the bar stays flat).

## Layout: workspaces · clock · status
- Left: workspace numbers, only workspaces with windows; active = red number (style is a setting:
  red number | inverted block | red dot under | sliding red pill | dots | app icons). Scroll switches, click focuses.
  Urgent workspace: breathing red glow. Then `App · short title`, fading out at the edge when long.
  Then media while playing: prev / play-pause / next + `Artist — Title`, and a full-width red progress line on the
  bar's bottom edge.
- Centre: clock (format is a setting: **HH:MM** | HH:MM:SS | date + time | stacked | 12 h). No red dot.
- Right: `CPU 12%  RAM 3.2G` (click: btop) · tray chevron (small popover) · Wi-Fi/Ethernet icon + live download
  rate · brightness icon + value (scroll changes it) · volume icon + value (scroll changes, middle-click mutes,
  right-click pavucontrol) · segmented 5-cell battery (red under 20%, charging sweep) · bell when unread.
  `DGPU` (red) while the RTX 4060 is awake and `TURBO` while turbo is on, only then.
- Thin line icons. Short tooltips. Soft hover pill that glides between items.

## Control centre (click the status cluster)
One column, phone style, with icons: Wi-Fi toggle + network list · Bluetooth + devices · mic mute · turbo ·
power profile (saver / balanced / performance) · RTX 4060 status (no switching) · volume + brightness sliders ·
media card · graphs (CPU, RAM, CPU temp, iGPU, RTX 4060 power/load while awake, network up/down, disk) ·
lock / logout / reboot / shutdown (click twice: first click arms, turns red) · bar settings (list with arrows,
applies live, saved to the config file). No presets.

## Notifications
Bell in the bar (only when unread) opens a notification panel. Popups stay with mako, top-right under the bar,
restyled to match (JetBrains Mono, frosted, 22 px), sliding in from the right.

## Motion
| Thing | Animation |
|---|---|
| Workspace switch | new number turns red with a springy scale pop (medium spring) |
| Workspace appears/disappears | row makes room, number fades in |
| Clock | changing digit rolls up |
| Numbers (CPU, rates, volume) | tween (count up/down) |
| Hover pill | glides between items |
| Panels | drop + fade in 180 ms; close with a quick fade |
| Media controls | slide in from the left |
| Progress line | smooth, flat red |
| Popups | slide in from the right |
| Urgent workspace | breathing glow |
| Login | cascade: left, centre, right |
