# Status

## Done
- Bar Lab: four rounds of design questions -> [bar-spec.md](bar-spec.md) (2026-11-10).
- nothing-bar slices 1–3: layer surface + tiny-skia + JetBrains Mono, animation core (springs, tweens, frame-driven
  only while moving), workspaces (pop, grow in/out, urgent breathe, click/scroll), app · title, clock roll, CPU/RAM,
  network rate, brightness, volume (PipeWire events), battery + charging sweep, DGPU/TURBO, hover pill, media
  (playerctl) with slide-in and progress line, live config file, hide/show, installer + autostart. 23 unit tests.
- Running on the LOQ in place of Waybar: ~5 MB RSS, draw ~0.2 ms/frame, ~0.5% of one core idle.
- Slice 4: control centre (click the status cluster): Wi-Fi/BT/mic/turbo tiles, networks and devices pages,
  power profile, RTX 4060 status, volume/brightness sliders (drag or scroll), media card, graphs (CPU, RAM, temp,
  iGPU, 4060 watts, network, disk), session buttons (click twice), bar settings page that writes the config.
  Frosted panel + click-catcher surface. 30 unit tests. Rendering verified on the LOQ; clicks not yet exercised
  by hand (no input injection on the box).

- Slice 5a: bell (unread count, event-driven via dbus-monitor) + notification panel with CLEAR ALL; mako popups
  restyled (JetBrains Mono, frosted, 22 px, slide in from the right). 31 unit tests.

- Tooltips: short, after 0.5 s on the same item, under it (own overlay surface, Hyprland fades it in).
- Slice 6: shapes (strip, floating, islands, island), positions (top, bottom, auto-hide, shrink), light material,
  workspace styles (red, block, dot, pill, dots), all in the settings page. Bar made 15% bigger: 32 px, 13.8 px text.
  Verified on the LOQ by screenshots; auto-hide's slide-back-in on hover not exercised (needs the cursor).

## Next
- Tray (StatusNotifierItem) behind the chevron: needs a D-Bus library (zbus) — to be agreed with the user.
- App icons as a workspace style (needs icon-theme lookup and SVG/PNG decoding).

## Blocked / found
- The RTX 4060 has no driver bound (nvidia module loaded, `nvidia-smi`: no devices) so it sits in D0 and never
  sleeps; the bar's DGPU indicator therefore shows all the time. Needs a look at the NVIDIA driver setup.
