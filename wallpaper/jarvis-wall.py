#!/usr/bin/env python3
"""Live JARVIS wallpaper for Hyprland.

Shows index.html (the jarvis-core.js holographic core) on the Wayland background layer and feeds it live
system telemetry. The core's state follows CPU load: idle -> thinking -> executing.
SIGUSR1 toggles pause (SUPER+F12 turbo mode sends it).
"""
import functools
import json
import os
import signal
import socket
import subprocess
import sys
import threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

# gtk4-layer-shell must load before libwayland-client, so re-exec once with it preloaded.
LAYER_SHELL = "/usr/lib/libgtk4-layer-shell.so"
if "libgtk4-layer-shell" not in os.environ.get("LD_PRELOAD", "") and os.path.exists(LAYER_SHELL):
    os.environ["LD_PRELOAD"] = (LAYER_SHELL + " " + os.environ.get("LD_PRELOAD", "")).strip()
    # Render on the integrated GPU only: Mesa's EGL, never NVIDIA's, so the dGPU can stay asleep.
    os.environ.setdefault("__EGL_VENDOR_LIBRARY_FILENAMES", "/usr/share/glvnd/egl_vendor.d/50_mesa.json")
    os.environ.setdefault("__GLX_VENDOR_LIBRARY_NAME", "mesa")
    os.execv(sys.executable, [sys.executable] + sys.argv)

import gi  # noqa: E402

# GTK4 composites WebKit's frames on the GPU; GTK3 copied them through the CPU (~35% of a core).
gi.require_version("Gtk", "4.0")
gi.require_version("Gtk4LayerShell", "1.0")
gi.require_version("WebKit", "6.0")
from gi.repository import GLib, Gtk, WebKit  # noqa: E402
from gi.repository import Gtk4LayerShell as LayerShell  # noqa: E402

ROOT = os.path.dirname(os.path.realpath(__file__))
TICK_S = 2
# CPU % at which the core changes state; the lower edge has hysteresis so it doesn't flicker.
LOAD_STATES = [(70, "executing"), (35, "thinking")]
HYSTERESIS = 8


def serve(root):
    """Serve the page from 127.0.0.1 only (ES modules don't load from file://)."""
    class Quiet(SimpleHTTPRequestHandler):
        def log_message(self, *_):
            pass

    httpd = ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(Quiet, directory=root))
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    return httpd.server_address[1]


class Stats:
    def __init__(self):
        self.prev = self._cpu_times()
        self.state = "idle"
        with open("/etc/os-release") as f:
            name = dict(l.rstrip().split("=", 1) for l in f if "=" in l).get("NAME", "LINUX").strip('"')
        self.model = f"{name} {os.uname().release.split('-')[0]}".upper()

    @staticmethod
    def _cpu_times():
        with open("/proc/stat") as f:
            v = [int(x) for x in f.readline().split()[1:]]
        return v[3] + v[4], sum(v)  # idle + iowait, total

    def cpu(self):
        idle, total = self._cpu_times()
        d_idle, d_total = idle - self.prev[0], total - self.prev[1]
        self.prev = (idle, total)
        return 0.0 if d_total <= 0 else 100.0 * (1 - d_idle / d_total)

    @staticmethod
    def ram():
        m = {}
        with open("/proc/meminfo") as f:
            for line in f:
                k, v = line.split(":", 1)
                m[k] = int(v.split()[0]) * 1024
        return m["MemTotal"] - m["MemAvailable"], m["MemTotal"]

    @staticmethod
    def link():
        try:  # UDP connect sends nothing; it only picks the outbound interface
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as s:
                s.connect(("1.1.1.1", 53))
                return {"up": True, "host": s.getsockname()[0], "local": True}
        except OSError:
            return {"up": False, "host": "", "local": True}

    def next_state(self, cpu):
        for limit, name in LOAD_STATES:
            if cpu >= limit or (self.state == name and cpu >= limit - HYSTERESIS):
                self.state = name
                return name
        self.state = "idle"
        return "idle"

    def snapshot(self):
        cpu = self.cpu()
        used, total = self.ram()
        tele = {"cpu": round(cpu, 1), "ramUsed": used, "ramTotal": total, "model": self.model, "link": self.link()}
        return tele, self.next_state(cpu)


class Wall:
    def __init__(self, url, loop, uncapped=False):
        self.stats = Stats()
        self.paused = False  # effective state sent to the page
        self.user_paused = False  # SIGUSR1 / turbo mode
        self.covered = False  # a fullscreen window hides the wallpaper

        win = Gtk.Window()
        LayerShell.init_for_window(win)
        LayerShell.set_layer(win, LayerShell.Layer.BACKGROUND)
        LayerShell.set_namespace(win, "jarvis-wallpaper")
        LayerShell.set_exclusive_zone(win, -1)  # draw under the bar too
        for edge in (LayerShell.Edge.TOP, LayerShell.Edge.BOTTOM, LayerShell.Edge.LEFT, LayerShell.Edge.RIGHT):
            LayerShell.set_anchor(win, edge, True)

        settings = WebKit.Settings()
        settings.set_enable_webgl(True)
        settings.set_hardware_acceleration_policy(WebKit.HardwareAccelerationPolicy.ALWAYS)
        # WebKit snaps rendering to the refresh divisor nearest 60 fps (72 on a 144 Hz panel), which keeps pacing
        # even. A custom fps cap needs it off. Must be set before the WebView exists.
        if uncapped:
            features = WebKit.Settings.get_all_features()
            for i in range(features.get_length()):
                if features.get(i).get_identifier() == "PreferPageRenderingUpdatesNear60FPS":
                    settings.set_feature_enabled(features.get(i), False)
        self.web = WebKit.WebView(settings=settings)
        self.web.set_background_color(_rgba("#05070b"))
        self.web.connect("notify::title", self._on_title)
        self.web.connect("load-changed", self._on_load)
        self.web.load_uri(url)

        win.set_child(self.web)
        win.connect("close-request", lambda *_: loop.quit())
        win.present()
        add_signal = getattr(_glib_unix(), "signal_add", None) or GLib.unix_signal_add
        add_signal(GLib.PRIORITY_DEFAULT, signal.SIGUSR1, self.toggle_pause)
        add_signal(GLib.PRIORITY_DEFAULT, signal.SIGTERM, lambda: loop.quit() or False)

    def js(self, code):
        self.web.evaluate_javascript(code, -1, None, None, None, None, None)

    def _on_load(self, _web, event):
        if event == WebKit.LoadEvent.FINISHED:
            self.watch_fullscreen()
            self._check_cover()
            self.tick()
            GLib.timeout_add_seconds(TICK_S, self.tick)

    def _on_title(self, web, _pspec):
        title = web.get_title() or ""
        if title.startswith("jarvis-wall:"):
            print(title, file=sys.stderr, flush=True)

    def tick(self):
        if not self.paused:
            tele, state = self.stats.snapshot()
            self.js(f"window.jarvisWall?.telemetry({json.dumps(tele)}); window.jarvisWall?.state({json.dumps(state)});")
        return True

    def toggle_pause(self):
        self.user_paused = not self.user_paused
        self._apply_pause()
        return True

    def _apply_pause(self):
        paused = self.user_paused or self.covered
        if paused != self.paused:
            self.paused = paused
            self.js(f"window.jarvisWall?.pause({json.dumps(paused)});")

    def watch_fullscreen(self):
        """WebKit keeps rendering under a fullscreen window; pause while the active workspace has one."""
        sig, run = os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"), os.environ.get("XDG_RUNTIME_DIR")
        if not sig or not run:
            return
        path = f"{run}/hypr/{sig}/.socket2.sock"
        triggers = (b"fullscreen>>", b"workspace>>", b"closewindow>>", b"movewindow>>", b"focusedmon>>")

        def listen():
            try:
                with socket.socket(socket.AF_UNIX) as s:
                    s.connect(path)
                    buf = b""
                    while chunk := s.recv(4096):
                        *lines, buf = (buf + chunk).split(b"\n")
                        if any(line.startswith(triggers) for line in lines):
                            GLib.idle_add(self._check_cover)
            except OSError as e:
                print(f"jarvis-wall: event socket closed: {e}", file=sys.stderr, flush=True)

        threading.Thread(target=listen, daemon=True).start()

    def _check_cover(self):
        try:
            ws = json.loads(subprocess.run(["hyprctl", "activeworkspace", "-j"], capture_output=True, text=True, timeout=2).stdout)
            self.covered = bool(ws.get("hasfullscreen"))
        except (OSError, ValueError, subprocess.TimeoutExpired):
            self.covered = False  # fail open: keep the wallpaper running
        self._apply_pause()
        return False


def _glib_unix():
    """GLibUnix replaced GLib.unix_* in PyGObject 3.52; fall back on older versions."""
    try:
        gi.require_version("GLibUnix", "2.0")
        from gi.repository import GLibUnix

        return GLibUnix
    except (ValueError, ImportError):
        return GLib


def _rgba(hex_color):
    from gi.repository import Gdk

    c = Gdk.RGBA()
    c.parse(hex_color)
    return c


def main():
    root = os.environ.get("JARVIS_WALL_DIR", ROOT)
    if not os.path.isfile(os.path.join(root, "jarvis-core.js")):
        sys.exit(f"jarvis-wall: {root}/jarvis-core.js missing (run install.sh with JARVIS_CORE=<jarvis-main/core/web>)")
    port = serve(root)
    # Measured on the LOQ's Intel iGPU at 1920x1080: full resolution, high quality = 72 fps, exactly half of 144 Hz,
    # so every frame is shown twice and pacing is even. 0 keeps that; a number (e.g. 120) is for stronger GPUs.
    fps = int(os.environ.get("JARVIS_WALL_FPS", "0"))
    quality = os.environ.get("JARVIS_WALL_QUALITY", "high")  # high | low
    hud = os.environ.get("JARVIS_WALL_HUD", "1")  # 0 hides the floating telemetry panels
    loop = GLib.MainLoop()
    Wall(f"http://127.0.0.1:{port}/index.html?fps={fps}&quality={quality}&hud={hud}", loop, uncapped=fps > 0)
    loop.run()


if __name__ == "__main__":
    main()
