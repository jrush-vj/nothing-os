#!/bin/bash
# User part of nothing-os: fonts + config. Run as your user after install-root.sh.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)

echo ">> fonts (OFL, from Google Fonts)"
fonts=~/.local/share/fonts/nothing-os
mkdir -p "$fonts"
base=https://raw.githubusercontent.com/google/fonts/main/ofl
curl -fsSL -o "$fonts/Doto.ttf"         "$base/doto/Doto%5BROND,wght%5D.ttf"
curl -fsSL -o "$fonts/SpaceGrotesk.ttf" "$base/spacegrotesk/SpaceGrotesk%5Bwght%5D.ttf"
fc-cache -f "$fonts" >/dev/null
fc-list : family | grep -q "^Doto" && echo "   Doto ok" || echo "   WARNING: Doto font not registered"

echo ">> nothing-bar"
cargo build --release --quiet --manifest-path "$here/bar/Cargo.toml"
install -Dm755 "$here/bar/target/release/nothing-bar" ~/.local/bin/nothing-bar
if [ ! -f ~/.config/nothing-bar/config ]; then
  install -Dm644 "$here/bar/config.example" ~/.config/nothing-bar/config
fi
if pgrep -x nothing-bar >/dev/null; then
  pkill -x nothing-bar; sleep 0.3; setsid -f ~/.local/bin/nothing-bar >/dev/null 2>&1
fi

echo ">> config"
if [ -d ~/.config/hypr ] && [ ! -f ~/.config/hypr/nothing/palette.lua ]; then
  mv ~/.config/hypr ~/.config/hypr.before-nothing-os
  echo "   old ~/.config/hypr moved to ~/.config/hypr.before-nothing-os"
fi
rm -rf ~/.config/hypr/nothing ~/.config/hypr/slate
mkdir -p ~/.config/hypr
cp -r "$here/hypr/." ~/.config/hypr/

if [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]; then
  hyprctl reload >/dev/null && hyprctl configerrors
fi
echo ">> done. Log out and back in (or reboot) for the full session."
