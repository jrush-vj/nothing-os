"""Checks the load -> state mapping and telemetry shape of wallpaper/jarvis-wall.py.

usage: python3 test/test_jarvis_wall.py   (needs python-gobject, webkit2gtk-4.1, gtk-layer-shell)
"""
import importlib.util
import os

path = os.path.join(os.path.dirname(__file__), "..", "wallpaper", "jarvis-wall.py")
spec = importlib.util.spec_from_file_location("jarvis_wall", path)
jw = importlib.util.module_from_spec(spec)
spec.loader.exec_module(jw)

s = jw.Stats()
# rising load crosses the plain thresholds
assert [s.next_state(c) for c in (10, 40, 75)] == ["idle", "thinking", "executing"]
# falling: hysteresis keeps the state until load drops HYSTERESIS below the threshold
assert s.next_state(65) == "executing"
assert s.next_state(60) == "thinking"
assert s.next_state(30) == "thinking"
assert s.next_state(20) == "idle"

tele, state = s.snapshot()
assert 0 <= tele["cpu"] <= 100 and 0 < tele["ramUsed"] < tele["ramTotal"]
assert state in ("idle", "thinking", "executing") and tele["model"]
print("ok:", state, tele["model"], f"cpu {tele['cpu']}%", tele["link"])
