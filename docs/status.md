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
- Size pass (2026-11-10): one SIZE setting scales bar, control centre and tooltips (default 130%: 36 px bar, 15.6 px
  text, ~14 px panel text, ~39 px rows). Swept every shape x material, workspace style, clock format, size and
  position by screenshot; fixed: workspace/app spacing, song title fading when it fits, panel gap above a bottom bar,
  restart race with the single-instance lock, DGPU shown only when a driver is bound and the GPU is active
  (control centre says NO DRIVER instead), ETHERNET on the networks row, graph tracks.

- Slice 7a (2026-11-21, code only): workspaces `slidefade 18%`; control centre dot-matrix reveal + label decode on open
  and on each page change (300 ms, off in turbo). On the LOQ: 33 tests pass, installed, frames captured mid-open
  (dots sweep from the bar's corner, labels decode with a red head); Hyprland reload with no config errors. Workspace
  slidefade not yet watched by eye; reveal frame cost not measured.

- Slices 7b–7f (2026-11-21): pop-ups as separate processes from the same binary (shell.rs runner: layer surface
  with keyboard focus, open/close motion, key repeat, toggle, PNG preview): notes, clip, player, wall; and the lock
  screen (lock.rs: ext-session-lock + PAM via libpam FFI). Shared fx.rs (dot screen, decode text, dot-matrix
  images via ImageMagick), text.rs (editor model). Keys: SUPER+N notes (editor moved to SUPER+SHIFT+N), SUPER+V
  clip, SUPER+P player, SUPER+W wallpaper; hypridle lock_cmd -> nothing-bar lock || hyprlock;
  misc:allow_session_lock_restore on. 48 tests (incl. real PAM rejecting a wrong password), Lua mock test passes.
  Verified live on the LOQ by screenshots: every widget with scripted keys, wallpaper transition, player with a fake
  MPRIS track (art, hover colour, pause), a real lock -> wrong password (PAM) -> unlock.
- Security review of 7b–7f (2026-11-21): FIX FIRST -> fixed: images are sniffed in Rust and only PNG/JPEG/GIF/WebP
  reach ImageMagick with an explicit coder (art URLs and clipboard data are untrusted); file:// art must be absolute;
  curl capped (10 MB, http/https only); the lock exits 1 (hyprlock takes over) when it cannot take its lock file;
  pop-up toggles only signal a pid > 1 whose comm matches; no /tmp fallback; password buffer pre-sized and wiped in
  full; PAM answers hidden prompts only, checks pam_acct_mgmt, and a check stuck for 30 s gives the prompt back.
  49 tests. Debug hooks exist only in the `probe` profile; the installed release binary has none (checked).
- Bar left side (2026-11-21): app name only (no window title); media in its own pill with the track title cut to 24
  chars. Progress line: faint track, whole pixels, re-reads the same MPRIS instance every 2 s (also while paused),
  keeps the length when Firefox drops it after a paused seek. 48 tests on the LOQ; seek-while-paused checked by
  screenshot. README rewritten around screenshots (docs/shots/).

## Next
- Not exercised by hand: real keyboard typing in the pop-ups (scripted keys use the same path after xkb), clipboard
  image entries, seeking by click, a second monitor while locked.
- Tray (StatusNotifierItem) behind the chevron: needs a D-Bus library (zbus) — to be agreed with the user.
- App icons as a workspace style (needs icon-theme lookup and SVG/PNG decoding).

## Blocked / found
- The RTX 4060 has no driver bound (nvidia module loaded, `nvidia-smi`: no devices) so it sits in D0 and never
  sleeps; the bar's DGPU indicator therefore shows all the time. Needs a look at the NVIDIA driver setup.
