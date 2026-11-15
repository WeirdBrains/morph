#!/usr/bin/env python3
"""Drive the desktop shell (`morph`) in Xvfb with xdotool and check that keys
reach pages and popups, that the shell's own keys work, that typing a
phrase searches, that find in page works beside the page, that the library
panel (bookmarks, history) opens beside the page and takes the keyboard, and
that a failed load shows Morph's error page.

    scripts/xvfb-check.py [--bin PATH] [--report FILE] [--no-wm]

Needs Xvfb and xdotool; uses openbox as the window manager when installed
(F11 needs one). Data goes to a temporary MORPH_HOME. The page is
tests/xvfb/keys.html, which reports its state as a JSON title; the shell
prints page events with MORPH_LOG_EVENTS=1.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PAGE = ROOT / "tests" / "xvfb" / "keys.html"
EVENT = re.compile(r'^morph-event tab=(\d+) (\w+)(?:\((.*)\))?')


class Morph:
    def __init__(self, binary, display, home):
        self.display = display
        self.home = Path(home)
        self.env = dict(os.environ, DISPLAY=display, MORPH_HOME=home, MORPH_LOG_EVENTS="1")
        self.env.pop("WAYLAND_DISPLAY", None)
        # A desktop of its own for the default-browser check: morph.desktop
        # and a `morph` on PATH (xdg-settings wants both), fresh config.
        xdg = Path(home) / "xdg"
        (xdg / "data" / "applications").mkdir(parents=True, exist_ok=True)
        (xdg / "config").mkdir(parents=True, exist_ok=True)
        (xdg / "bin").mkdir(parents=True, exist_ok=True)
        shutil.copy(ROOT / "packaging" / "linux" / "morph.desktop", xdg / "data" / "applications" / "morph.desktop")
        if not (xdg / "bin" / "morph").exists():
            (xdg / "bin" / "morph").symlink_to(Path(binary).resolve())
        # xdg-settings can take seconds on a real desktop; for Morph it takes
        # two here, so a window that waited for it would miss keys (the
        # checks' own calls, without MORPH_HOME, are not slowed).
        real = shutil.which("xdg-settings", path=os.environ.get("PATH"))
        if real and not (xdg / "bin" / "xdg-settings").exists():
            (xdg / "bin" / "xdg-settings").write_text(f'#!/bin/sh\n[ -n "$MORPH_HOME" ] && sleep 2\nexec {real} "$@"\n')
            (xdg / "bin" / "xdg-settings").chmod(0o755)
        self.env.update(XDG_DATA_HOME=str(xdg / "data"), XDG_CONFIG_HOME=str(xdg / "config"), PATH=f"{xdg / 'bin'}:{os.environ.get('PATH', '')}")
        for var in ("BROWSER", "XDG_CURRENT_DESKTOP", "DESKTOP_SESSION"):
            self.env.pop(var, None)
        self.log_path = Path(home) / "morph.log"
        self.log = open(self.log_path, "w")
        self.proc = subprocess.Popen([binary, PAGE.as_uri()], env=self.env, stdout=self.log, stderr=subprocess.STDOUT)
        self.read_at = 0

    def lines(self, pattern):
        """Log lines matching `pattern` (a regex)."""
        rx = re.compile(pattern)
        return [line for line in self.log_path.read_text(errors="replace").splitlines() if rx.search(line)]

    def events(self):
        out = []
        for line in self.log_path.read_text(errors="replace").splitlines():
            m = EVENT.match(line)
            if m:
                out.append((int(m.group(1)), m.group(2), m.group(3)))
        return out

    def xdo(self, *args):
        return subprocess.run(["xdotool", *args], env=self.env, capture_output=True, text=True).stdout.strip()

    def key(self, *keys, delay=0.35):
        for k in keys:
            self.xdo("key", "--clearmodifiers", k)
            time.sleep(delay)

    def type(self, text):
        self.xdo("type", "--delay", "60", text)
        time.sleep(0.5)

    def window(self, name):
        ids = self.xdo("search", "--onlyvisible", "--name", name).split()
        return ids[0] if ids else None

    def own_window(self):
        """This process's visible window (two Morphs share a display)."""
        ids = self.xdo("search", "--onlyvisible", "--pid", str(self.proc.pid)).split()
        return ids[-1] if ids else None

    def geometry(self, win):
        out = self.xdo("getwindowgeometry", "--shell", win)
        vals = dict(line.split("=", 1) for line in out.splitlines() if "=" in line)
        return int(vals["X"]), int(vals["Y"]), int(vals["WIDTH"]), int(vals["HEIGHT"])

    def click(self, win, x, y):
        self.xdo("mousemove", "--window", win, str(x), str(y))
        time.sleep(0.15)
        self.xdo("click", "1")
        time.sleep(0.5)

    def stop(self):
        self.proc.terminate()
        try:
            self.proc.wait(5)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def state(morph, popup=False, tab=None):
    """The newest page state the main page (or popup) reported."""
    kind = "PopupTitleChanged" if popup else "TitleChanged"
    for t, name, arg in reversed(morph.events()):
        if name != kind or not arg or (tab is not None and t != tab):
            continue
        try:
            title = json.loads(arg)
        except json.JSONDecodeError:
            continue
        if title[:1] in ("M", "P") and title[1:2] == "{":
            return json.loads(title[1:])
    return None


def page_takes_key(m, key, code, **kw):
    """Press `key` once; the page must report `code`."""
    m.key(key)
    return wait_for(lambda: (state(m, **kw) or {}).get("c") == code, 3)


def wait_for(fn, timeout=6.0):
    end = time.time() + timeout
    value = fn()
    while not value and time.time() < end:
        time.sleep(0.2)
        value = fn()
    return value


class Results:
    def __init__(self):
        self.rows = []

    def check(self, area, name, ok, detail=""):
        self.rows.append((area, name, bool(ok), detail))
        print(f"{'PASS' if ok else 'FAIL'}  {area:7} {name}{'  (' + detail + ')' if detail and not ok else ''}", flush=True)

    def markdown(self):
        lines = ["| Where | Check | Result |", "|---|---|---|"]
        for area, name, ok, detail in self.rows:
            lines.append(f"| {area} | {name} | {'pass' if ok else 'FAIL: ' + detail} |")
        return "\n".join(lines)


def page_keys(m, r, area, win, popup, top):
    """Keys inside a page: editing, focus, clipboard, scrolling, keypad."""
    st = lambda: state(m, popup=popup)  # noqa: E731
    m.click(win, 60, top + 18)  # into #a
    r.check(area, "click focuses the input", (st() or {}).get("f") == "a", str(st()))
    m.type("hello world")
    r.check(area, "typing", (st() or {}).get("a") == "hello world", str(st()))
    if not popup:
        m.xdo("mousemove", "--window", win, "300", "50")  # over the shell's toolbar
        m.type("!")
        r.check(area, "typing with the pointer over the shell", (st() or {}).get("a") == "hello world!", str(st()))
        m.xdo("mousemove", "--window", win, "300", str(top + 300))
        m.key("BackSpace")
    m.key("BackSpace")
    r.check(area, "Backspace", (st() or {}).get("a") == "hello worl", str(st()))
    m.key("Home")
    r.check(area, "Home in a field", (st() or {}).get("s") == 0, str(st()))
    m.key("Delete")
    r.check(area, "Delete", (st() or {}).get("a") == "ello worl", str(st()))
    m.key("End")
    r.check(area, "End in a field", (st() or {}).get("s") == 9, str(st()))
    m.key("Left", "Left")
    r.check(area, "Left arrow", (st() or {}).get("s") == 7, str(st()))
    m.key("Right")
    r.check(area, "Right arrow", (st() or {}).get("s") == 8, str(st()))
    m.key("ctrl+a", "ctrl+c", "Tab")
    r.check(area, "Ctrl+A, Tab moves focus", (st() or {}).get("f") == "b", str(st()))
    m.key("ctrl+v")
    r.check(area, "Ctrl+C / Ctrl+V", (st() or {}).get("b") == "ello worl", str(st()))
    m.key("shift+Tab")
    r.check(area, "Shift+Tab moves focus back", (st() or {}).get("f") == "a", str(st()))
    m.key("ctrl+a", "ctrl+x")
    r.check(area, "Ctrl+X", (st() or {}).get("a") == "", str(st()))
    m.key("ctrl+z")
    r.check(area, "Ctrl+Z", (st() or {}).get("a") == "ello worl", str(st()))
    m.key("End", "Return")
    r.check(area, "Enter reaches the page", (st() or {}).get("c") == "Enter", str(st()))
    m.key("Escape")
    r.check(area, "Escape reaches the page", (st() or {}).get("c") == "Escape", str(st()))
    if popup:
        m.key("ctrl+f")
        s = st() or {}
        r.check(area, "Ctrl+F reaches a popup's page (popups have no find bar)", s.get("c") == "KeyF" and s.get("m") == "C", str(s))
    m.key("KP_1")
    s = st() or {}
    r.check(area, "keypad key, Num Lock off", s.get("c") == "Numpad1", str(s))
    m.key("Num_Lock")
    m.key("KP_7", "KP_Add", "KP_2")
    s = st() or {}
    r.check(area, "keypad digits and +, Num Lock on", s.get("a", "").endswith("7+2"), str(s))
    m.key("KP_Enter")
    s = st() or {}
    r.check(area, "keypad Enter", s.get("c") == "NumpadEnter", str(s))
    m.key("Num_Lock")

    # Scrolling needs the page, not a field, focused.
    m.click(win, 300, top + 300)
    m.key("Escape")
    y0 = (st() or {}).get("y", -1)
    m.key("Next")
    y1 = (st() or {}).get("y", -1)
    r.check(area, "Page Down scrolls", y1 > y0 >= 0, f"{y0} -> {y1}")
    m.key("Prior")
    y2 = (st() or {}).get("y", -1)
    r.check(area, "Page Up scrolls back", y2 < y1, f"{y1} -> {y2}")
    m.key("space")
    y3 = (st() or {}).get("y", -1)
    r.check(area, "Space scrolls down", y3 > y2, f"{y2} -> {y3}")
    m.key("shift+space")
    y4 = (st() or {}).get("y", -1)
    r.check(area, "Shift+Space scrolls up", y4 < y3, f"{y3} -> {y4}")
    m.key("Down", "Down", "Down")
    y5 = (st() or {}).get("y", -1)
    r.check(area, "Down arrow scrolls", y5 > y4, f"{y4} -> {y5}")
    m.key("Up")
    y6 = (st() or {}).get("y", -1)
    r.check(area, "Up arrow scrolls", y6 < y5, f"{y5} -> {y6}")
    m.key("End", delay=1.0)
    y7 = (st() or {}).get("y", -1)
    r.check(area, "End scrolls to the bottom", y7 > 4000, f"{y7}")
    m.key("Home", delay=1.0)
    y8 = (st() or {}).get("y", -1)
    r.check(area, "Home scrolls to the top", y8 == 0, f"{y8}")
    id0 = (st() or {}).get("id")
    m.key("F5", delay=1.5)
    r.check(area, "F5 reloads", wait_for(lambda: (st() or {}).get("id") not in (None, id0)), str(st()))
    id1 = (st() or {}).get("id")
    m.click(win, 300, top + 300)
    m.key("ctrl+r", delay=1.5)
    r.check(area, "Ctrl+R reloads", wait_for(lambda: (st() or {}).get("id") not in (None, id1)), str(st()))


def find_matches(m):
    """Match counts from FindResult events, oldest first."""
    return [int(x) for line in m.lines(r"FindResult \{ matches: \d+ \}") for x in re.findall(r"matches: (\d+)", line)]


def find_in_page(m, r, win, top):
    """Ctrl+F opens the find bar above the page (the page shrinks rather
    than being covered), typing searches, Enter/F3 move, Escape closes and
    gives the page the keyboard back. The page never sees these keys."""
    st = lambda: state(m, tab=1)  # noqa: E731
    m.click(win, 300, top + 300)
    m.key("Escape")
    s0 = st() or {}
    n0, h0 = s0.get("n"), s0.get("h", 0)
    m.key("ctrl+f", delay=0.8)
    h1 = wait_for(lambda: (st() or {}).get("h", 0) < h0 and (st() or {}).get("h"))
    r.check("find", "Ctrl+F opens the find bar beside the page (page shrinks)", h1 and h1 < h0, f"{h0} -> {(st() or {}).get('h')}")
    r.check("find", "the page never saw Ctrl+F", (st() or {}).get("n") == n0, f"{n0} vs {(st() or {}).get('n')}")
    before = len(find_matches(m))
    m.type("needle")
    found = wait_for(lambda: find_matches(m)[before:])
    r.check("find", "typing searches the page (2 matches)", found and found[-1] == 2, str(found))
    r.check("find", "the page never saw the find text", (st() or {}).get("n") == n0, f"{n0} vs {(st() or {}).get('n')}")
    before = len(find_matches(m))
    m.key("Return")
    r.check("find", "Enter goes to the next match", wait_for(lambda: find_matches(m)[before:]), "no FindResult")
    before = len(find_matches(m))
    m.key("F3")
    r.check("find", "F3 goes to the next match", wait_for(lambda: find_matches(m)[before:]), "no FindResult")
    before = len(find_matches(m))
    m.key("shift+F3")
    r.check("find", "Shift+F3 goes to the previous match", wait_for(lambda: find_matches(m)[before:]), "no FindResult")
    before = len(find_matches(m))
    m.key("ctrl+a")
    m.type("nomatchzz")
    none = wait_for(lambda: [x for x in find_matches(m)[before:] if x == 0])
    r.check("find", "text that is not there finds nothing", none, str(find_matches(m)[before:]))
    r.check("find", "the page never saw the find bar's keys", (st() or {}).get("n") == n0, f"{n0} vs {(st() or {}).get('n')}")
    m.key("Escape", delay=0.8)
    h2 = wait_for(lambda: (st() or {}).get("h", 0) == h0 and (st() or {}).get("h"))
    r.check("find", "Escape closes the find bar (page grows back)", h2 == h0, f"{h1} -> {(st() or {}).get('h')}")
    r.check("find", "after Escape the page has the keyboard again", page_takes_key(m, "x", "KeyX", tab=1), str(st()))
    n1 = (st() or {}).get("n")
    m.key("ctrl+g", delay=0.8)
    h3 = wait_for(lambda: (st() or {}).get("h", 0) < h0 and (st() or {}).get("h"))
    r.check("find", "Ctrl+G opens the find bar too", h3 and h3 < h0, f"{h0} -> {(st() or {}).get('h')}")
    m.key("Escape", delay=0.8)
    wait_for(lambda: (st() or {}).get("h", 0) == h0)
    r.check("find", "the page never saw Ctrl+G or Escape", (st() or {}).get("n") == n1, f"{n1} vs {(st() or {}).get('n')}")


def bookmarks_file(m):
    path = m.home / "profiles" / "default" / "bookmarks.json"
    return path.read_text(errors="replace") if path.exists() else ""


def library_panel(m, r, win, top):
    """Ctrl+D bookmarks the page; Ctrl+Shift+O and Ctrl+H open the library
    beside the page (the page gets narrower, never covered); its search takes
    the keyboard and Enter opens the first match; the same key or Escape
    closes it and the page has the keyboard again."""
    st = lambda: state(m, tab=1)  # noqa: E731
    url = PAGE.as_uri()
    m.click(win, 300, top + 300)
    s0 = st() or {}
    n0, w0 = s0.get("n"), s0.get("w", 0)
    m.key("ctrl+d", delay=0.8)
    r.check("library", "Ctrl+D bookmarks the page", wait_for(lambda: url in bookmarks_file(m)), bookmarks_file(m)[:200])
    m.key("ctrl+shift+o", delay=0.8)
    w1 = wait_for(lambda: (st() or {}).get("w", 0) < w0 and (st() or {}).get("w"))
    r.check("library", "Ctrl+Shift+O opens bookmarks beside the page (page narrows)", w1 and w1 < w0, f"{w0} -> {(st() or {}).get('w')}")
    m.type("keys.html")
    r.check("library", "the page never saw Ctrl+D, Ctrl+Shift+O or the search text", (st() or {}).get("n") == n0, f"{n0} vs {(st() or {}).get('n')}")
    id0 = (st() or {}).get("id")
    m.key("Return", delay=1.5)
    r.check("library", "Enter in the search opens the bookmark", wait_for(lambda: (st() or {}).get("id") not in (None, id0)), str(st()))
    m.key("ctrl+h", delay=0.8)
    r.check("library", "Ctrl+H switches the open library to history", (st() or {}).get("w") == w1, f"{w1} vs {(st() or {}).get('w')}")
    m.type("keys")
    id1 = (st() or {}).get("id")
    m.key("Return", delay=1.5)
    r.check("library", "Enter in the history search opens the visit", wait_for(lambda: (st() or {}).get("id") not in (None, id1)), str(st()))
    m.key("ctrl+h", delay=0.8)
    w2 = wait_for(lambda: (st() or {}).get("w") == w0 and w0)
    r.check("library", "Ctrl+H again closes the library (page widens)", w2 == w0, f"{w1} -> {(st() or {}).get('w')}")
    m.key("ctrl+h", delay=0.8)
    wait_for(lambda: (st() or {}).get("w", 0) < w0)
    m.key("Escape", delay=0.8)
    w3 = wait_for(lambda: (st() or {}).get("w") == w0 and w0)
    r.check("library", "Escape in the library closes it", w3 == w0, f"{(st() or {}).get('w')}")
    r.check("library", "after closing, the page has the keyboard", page_takes_key(m, "q", "KeyQ", tab=1), str(st()))
    m.key("ctrl+d", delay=0.8)
    r.check("library", "Ctrl+D again removes the bookmark", wait_for(lambda: url not in bookmarks_file(m)), bookmarks_file(m)[:200])


def catalog(m):
    try:
        return json.loads((m.home / "profiles.json").read_text())
    except (OSError, json.JSONDecodeError):
        return {}


def profiles_panel(m, r, win, top):
    """Ctrl+Shift+M opens the profiles panel ready for a new name; Enter
    creates the profile and switches the window to it (a fresh blank tab
    whose address bar takes typing); Ctrl+Shift+M again switches back."""
    m.xdo("windowactivate", "--sync", win)
    m.click(win, 300, top + 300)
    w0 = (state(m) or {}).get("w", 0)
    m.key("ctrl+shift+m", delay=0.8)
    narrowed = wait_for(lambda: (state(m) or {}).get("w", 0) < w0)
    r.check("profile", "Ctrl+Shift+M opens the profiles panel beside the page", narrowed, f"{w0} -> {(state(m) or {}).get('w')}")
    m.type("Work")
    m.key("Return", delay=2.0)
    cat = catalog(m)
    new = [p["id"] for p in cat.get("profiles", []) if p.get("name") == "Work"]
    r.check("profile", "Enter creates the profile", new, str(cat))
    r.check("profile", "and the window switches to it", new and cat.get("current") == new[0], str(cat))
    r.check("profile", "the new profile starts with a blank tab", wait_for(lambda: m.xdo("getwindowname", win) == "Morph"), m.xdo("getwindowname", win))
    before = len(m.events())
    m.type(PAGE.as_uri() + "?work")
    m.key("Return", delay=1.5)
    loaded = wait_for(lambda: any(n == "TitleChanged" and "M{" in (a or "") for _, n, a in m.events()[before:]))
    r.check("profile", "its address bar takes typing and loads pages", loaded, m.xdo("getwindowname", win))
    data = m.home / "profiles" / (new[0] if new else "?") / "webkit" / "data"
    r.check("profile", "its web data lives in its own directory", wait_for(lambda: data.exists()), str(data))


def settings_file(m):
    cur = catalog(m).get("current", "default")
    path = m.home / "profiles" / cur / "settings.json"
    return path.read_text(errors="replace") if path.exists() else ""


def settings_panel(m, r, win, top):
    """Ctrl+, opens the settings beside the page and takes the keyboard:
    Left moves the tabs into a list left of the page (saved for the profile),
    Escape closes the panel and the page has the keyboard again."""
    m.xdo("windowactivate", "--sync", win)
    m.click(win, 300, top + 300)
    s0 = state(m) or {}
    w0, h0 = s0.get("w", 0), s0.get("h", 0)
    m.key("ctrl+comma", delay=0.8)
    r.check("settings", "Ctrl+, opens the settings beside the page", wait_for(lambda: (state(m) or {}).get("w", 0) < w0), f"{w0} -> {(state(m) or {}).get('w')}")
    if shutil.which("xdg-settings"):
        env = {k: v for k, v in m.env.items() if k != "MORPH_HOME"}
        get = lambda: subprocess.run(["xdg-settings", "get", "default-web-browser"], env=env, capture_output=True, text=True).stdout.strip()  # noqa: E731
        m.key("d", delay=0.3)
        settings_check = time.time()
    m.key("Left", delay=0.8)
    r.check("settings", "Left moves the tabs to the left (saved)", wait_for(lambda: '"left"' in settings_file(m)), settings_file(m))
    r.check("settings", "the page grows taller without the tab row", wait_for(lambda: (state(m) or {}).get("h", 0) == h0 + 34), f"{h0} -> {(state(m) or {}).get('h')}")
    if shutil.which("xdg-settings"):
        # The slow xdg-settings is still running: the keys above did not wait.
        r.check("settings", "the arrow keys did not wait for xdg-settings", time.time() - settings_check < 4.0, f"{time.time() - settings_check:.1f}s")
        r.check("settings", "d makes Morph the default browser (xdg-settings)", wait_for(lambda: get() == "morph.desktop", 15), get())
    m.key("Escape", delay=0.8)
    side = wait_for(lambda: (state(m) or {}).get("w") == w0 - 220 and w0 - 220)
    r.check("settings", "Escape closes it; the tab list takes 220 px", side, f"{w0} -> {(state(m) or {}).get('w')}")
    r.check("settings", "after closing, the page has the keyboard", page_takes_key(m, "z", "KeyZ"), str(state(m)))
    return w0, h0


def settings_relaunch(m, r, w0, h0):
    """The profile's settings apply at start; Ctrl+, then Up puts the tabs
    back on top."""
    win = wait_for(lambda: m.window("Morph"), 20)
    r.check("settings", "relaunch opens the window", win)
    ok = wait_for(lambda: (state(m) or {}).get("w") == w0 - 220 and (state(m) or {}).get("h") == h0 + 34, 20)
    r.check("settings", "the saved left tab list applies at start", ok, str(state(m)))
    if not win:
        return
    m.xdo("windowactivate", "--sync", win)
    m.click(win, 600, 300)
    m.key("ctrl+comma", delay=0.8)
    m.key("Up", delay=0.8)
    m.key("Escape", delay=0.8)
    back = wait_for(lambda: (state(m) or {}).get("w") == w0 and (state(m) or {}).get("h") == h0)
    r.check("settings", "Up puts the tabs back on top", back and '"top"' in settings_file(m), f"{state(m)} {settings_file(m)}")


def frameless(m, r):
    """With a compositor running, the window is frameless: no WM frame, a
    thin resize border, the tab row drags the window and a double click on
    it maximizes; the page keeps the keyboard; the close button closes."""
    win = wait_for(lambda: m.window("Morph"), 20)
    r.check("frame", "window opens with a compositor", win)
    if not win or not wait_for(lambda: state(m), 20):
        r.check("frame", "page loads", False)
        return
    hints = m.xdo("getwindowname", win) and subprocess.run(["xprop", "-id", win, "_MOTIF_WM_HINTS"], env=m.env, capture_output=True, text=True).stdout
    r.check("frame", "asks the window manager for no frame", "0x2, 0x0, 0x0" in hints, hints.strip())
    s0 = state(m) or {}
    r.check("frame", "a resize border surrounds the page", s0.get("w") == 1190 and s0.get("h") == 714, str(s0))
    m.click(win, 300, 300)
    m.key("k")
    r.check("frame", "the page has the keyboard", wait_for(lambda: (state(m) or {}).get("c") == "KeyK", 3), str(state(m)))
    x0, y0, _, _ = m.geometry(win)
    m.xdo("mousemove", "--window", win, "700", "20")
    time.sleep(0.2)
    m.xdo("mousedown", "1")
    time.sleep(0.3)
    for step in range(1, 6):
        m.xdo("mousemove", "--window", win, str(700 + step * 10), str(20 + step * 8))
        time.sleep(0.1)
    m.xdo("mouseup", "1")
    time.sleep(0.8)
    x1, y1, _, _ = m.geometry(win)
    r.check("frame", "dragging the tab row moves the window", (x1, y1) != (x0, y0), f"{(x0, y0)} -> {(x1, y1)}")
    m.xdo("mousemove", "--window", win, "700", "20")
    time.sleep(0.2)
    m.xdo("click", "--repeat", "2", "--delay", "80", "1")
    grown = wait_for(lambda: (state(m) or {}).get("w", 0) > 1300)
    r.check("frame", "double-clicking the tab row maximizes", grown, str(state(m)))
    m.xdo("mousemove", "--window", win, "700", "18")
    time.sleep(0.2)
    m.xdo("click", "--repeat", "2", "--delay", "80", "1")
    r.check("frame", "double-clicking again restores", wait_for(lambda: (state(m) or {}).get("w") == 1190), str(state(m)))
    _, _, w, _ = m.geometry(win)
    m.click(win, w - 22, 18)
    r.check("frame", "the close button closes the window", wait_for(lambda: m.proc.poll() is not None, 5), "still running")


def single_instance(m, r, binary):
    """A second `morph URL` for the same home hands the URL to the running
    window (a new tab there) and exits."""
    before = len(m.events())
    start = time.time()
    try:
        done = subprocess.run([binary, PAGE.as_uri() + "?handed"], env=m.env, capture_output=True, text=True, timeout=15)
        code, took = done.returncode, time.time() - start
    except subprocess.TimeoutExpired:
        code, took = None, 15.0
    r.check("single", "a second launch exits at once", code == 0 and took < 5, f"exit {code} after {took:.1f}s")
    # The running window had one tab; the handed URL (the keys page) loads in a second.
    handed = wait_for(lambda: any(t > 1 and n == "TitleChanged" and "M{" in (a or "") for t, n, a in m.events()[before:]))
    r.check("single", "and its URL opens in a new tab of the running window", handed, str(m.events()[before:][-4:]))
    r.check("single", "the first Morph keeps running", m.proc.poll() is None)


def identity_file(m, profile=None):
    profile = profile or catalog(m).get("current", "default")
    try:
        return json.loads((m.home / "profiles" / profile / "identity" / "identity.json").read_text())
    except (OSError, json.JSONDecodeError):
        return {}


def open_identity(m, r, area):
    """Ctrl+Shift+L opens the identity panel beside the page."""
    win = wait_for(m.own_window, 20)
    m.xdo("windowactivate", "--sync", win)
    m.click(win, 300, 76 + 300)
    w0 = (state(m) or {}).get("w", 0)
    m.key("ctrl+shift+l", delay=1.0)
    r.check(area, "Ctrl+Shift+L opens the identity panel beside the page", wait_for(lambda: (state(m) or {}).get("w", 0) < w0), f"{w0} -> {(state(m) or {}).get('w')}")
    return win, w0


def fill(m, *values):
    """Type each value into the panel's fields; Enter moves to the next, the
    last Enter submits."""
    for value in values:
        m.key("ctrl+a", delay=0.2)
        m.type(value)
        m.key("Return", delay=0.8)


PASSWORD = "xvfb-profile-pass"
BACKUP_PASSWORD = "xvfb-backup-pass"


def identity_panel(m, r):
    """Onboarding and backups on the running Morph: 1 creates an identity
    (password twice), Enter past the recovery words, b saves a backup,
    Escape closes the panel and the page has the keyboard again."""
    win, w0 = open_identity(m, r, "identity")
    m.key("1", delay=0.8)
    fill(m, PASSWORD, PASSWORD)
    made = wait_for(lambda: identity_file(m).get("user_id"))
    r.check("identity", "1, a password twice: the profile has an identity", made, str(identity_file(m)))
    m.key("Return", delay=0.8)
    backup = m.home / "identity-backup.txt"
    m.key("b", delay=0.8)
    fill(m, PASSWORD, BACKUP_PASSWORD, str(backup))
    r.check("identity", "b saves a backup file", wait_for(lambda: backup.exists()), str(backup))
    r.check("identity", "only its owner can read it", backup.exists() and (backup.stat().st_mode & 0o777) == 0o600, oct(backup.stat().st_mode) if backup.exists() else "")
    r.check("identity", "it does not hold the profile password", backup.exists() and PASSWORD not in backup.read_text(errors="replace"))
    m.key("Escape", delay=0.8)
    r.check("identity", "Escape closes the panel", wait_for(lambda: (state(m) or {}).get("w") == w0), f"{w0} vs {(state(m) or {}).get('w')}")
    r.check("identity", "after closing, the page has the keyboard", page_takes_key(m, "z", "KeyZ"), str(state(m)))
    return made, backup


def identity_relaunch(m, r):
    """With an identity the library is encrypted; without a keyring (as
    under Xvfb) the next launch opens the library asking for the password.
    The password unlocks it; Escape closes it; the page has the keyboard."""
    win = wait_for(m.own_window, 20)
    r.check("identity", "after a relaunch the page loads", wait_for(lambda: state(m), 20))
    w = (state(m) or {}).get("w", 0)
    m.xdo("windowactivate", "--sync", win)
    r.check("identity", "the locked library asks for the password beside the page", w and w < 1190, str(w))
    m.type(PASSWORD)
    m.key("Return", delay=1.0)
    r.check("identity", "the identity's password unlocks it", wait_for(lambda: m.lines(r"morph-event library unlocked")), "not unlocked")
    m.key("Escape", delay=1.0)
    r.check("identity", "Escape closes it", wait_for(lambda: (state(m) or {}).get("w", 0) > w), str((state(m) or {}).get("w")))
    r.check("identity", "and the page has the keyboard", page_takes_key(m, "z", "KeyZ"), str(state(m)))


def identity_restore(r, binary, display, user_id, backup):
    """A second home restores the backup: 3, the file, its password and a
    new password give it the same identity."""
    m2 = Morph(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-restore-"))
    try:
        r.check("restore", "a Morph with another home opens its own window", wait_for(lambda: state(m2), 20))
        open_identity(m2, r, "restore")
        m2.key("3", delay=0.8)
        fill(m2, str(backup), BACKUP_PASSWORD, PASSWORD)
        got = wait_for(lambda: identity_file(m2, "default").get("user_id"))
        r.check("restore", "3, the backup and its password restore the same identity", got and got == user_id, f"{got} vs {user_id}")
    finally:
        m2.stop()


def identity_link(m, r, binary, display, user_id):
    """Device linking: a fresh Morph (another home) offers a link with 4; the
    first adds it (a, password, link); both show the same code, Enter on
    each confirms, and the new one chooses its password and gets the
    identity."""
    m3 = Morph(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-link-"))
    win, w0 = None, 0
    try:
        r.check("link", "a new device (another home) opens", wait_for(lambda: state(m3), 20))
        new_win, _ = open_identity(m3, r, "link")
        m3.key("4", delay=1.5)
        offer = wait_for(lambda: [line.split("offer=", 1)[1].strip() for line in m3.lines(r"morph-event link offer=")])
        r.check("link", "4 shows a link (and its QR code)", offer, "no offer logged")
        if not offer:
            return
        win, w0 = open_identity(m, r, "link")
        m.key("a", delay=0.8)
        fill(m, PASSWORD, offer[0])
        time.sleep(3.0)
        m.key("Return", delay=1.0)
        m3.xdo("windowactivate", "--sync", new_win)
        m3.key("Return", delay=3.0)
        fill(m3, PASSWORD)
        got = wait_for(lambda: identity_file(m3, "default").get("user_id"), 20)
        r.check("link", "after both confirm, the new device has the same identity", got and got == user_id, f"{got} vs {user_id}")
        r.check("link", "and it knows it came over a link", "link" in json.dumps(identity_file(m3, "default").get("source", "")).lower(), str(identity_file(m3, "default").get("source")))
    finally:
        m3.stop()
    if not win:
        return
    # Escape backs out of the finished link, then closes the panel.
    m.xdo("windowactivate", "--sync", win)
    for _ in range(3):
        if (state(m) or {}).get("w") == w0:
            break
        m.key("Escape", delay=0.8)
    r.check("link", "Escape closes the panel on the first device", wait_for(lambda: (state(m) or {}).get("w") == w0), f"{w0} vs {(state(m) or {}).get('w')}")


def bridge_round_trip(m, r, binary):
    """The page bridge: a page asks Rust through window.MorphBridge (ping,
    an echoed event, a bad message) and gets each answer back."""
    before = len(m.events())
    page = (ROOT / "tests" / "xvfb" / "bridge.html").as_uri()
    subprocess.run([binary, page], env=m.env, capture_output=True, timeout=15)

    def answer():
        for _, n, a in m.events()[before:]:
            if n == "TitleChanged" and a and '"B{' in a:
                return json.loads(json.loads(a)[1:])
        return None

    got = wait_for(answer, 10) or {}
    r.check("bridge", "pages have window.MorphBridge", got.get("has") is True, str(got))
    r.check("bridge", "ping comes back as pong", got.get("ping") == "pong", str(got))
    r.check("bridge", "a message comes back with its name and payload", got.get("echo") == "ok:xvfb:7", str(got))
    r.check("bridge", "a bad message is rejected with Rust's error", got.get("error") == "rejected", str(got))


def error_page(m, r, win, top):
    """A load that fails shows Morph's error page (the web view is hidden)
    and keeps the shell's keys working: F5 tries again, Alt+Left goes back."""
    # A port nothing listens on (found by binding and closing it), so the
    # connection is refused.
    import socket
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = sock.getsockname()[1]
    bad = f"http://127.0.0.1:{port}/"
    m.click(win, 300, top + 300)
    before = len(m.lines(r"LoadFailed"))
    m.key("ctrl+l", delay=0.6)
    m.type(bad)
    m.key("Return", delay=1.5)
    failed = wait_for(lambda: m.lines(r"LoadFailed")[before:])
    r.check("error", "an unreachable site reports a failed load", failed and "Unreachable" in failed[-1], str(failed))
    m.key("F5", delay=1.5)
    r.check("error", "F5 on the error page tries again", wait_for(lambda: len(m.lines(r"LoadFailed")) >= before + 2), str(m.lines(r"LoadFailed")[before:]))
    m.key("alt+Left", delay=1.0)
    # Back shows the page under the error page again, with the keyboard.
    m.key("y")
    back = wait_for(lambda: (state(m, tab=2) or {}).get("c") == "KeyY")
    r.check("error", "Alt+Left leaves the error page for the page under it", back, f"{m.xdo('getwindowname', win)} {state(m, tab=2)}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=str(Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "morph"))
    ap.add_argument("--report")
    ap.add_argument("--no-wm", action="store_true")
    args = ap.parse_args()

    display = ":" + str(70 + os.getpid() % 200)
    xvfb = subprocess.Popen(["Xvfb", display, "-screen", "0", "1600x1000x24", "-ac", "-noreset"], stderr=subprocess.DEVNULL)
    time.sleep(1.5)
    wm = None
    if not args.no_wm and shutil.which("openbox"):
        wm = subprocess.Popen(["openbox"], env=dict(os.environ, DISPLAY=display), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(1.0)
    home = tempfile.mkdtemp(prefix="morph-xvfb-")
    m = Morph(args.bin, display, home)
    r = Results()
    try:
        win = wait_for(lambda: m.window("Morph"), 20)
        r.check("shell", "window opens", win)
        r.check("shell", "page loads", wait_for(lambda: state(m), 20))
        if not win:
            return finish(r, args)
        # The page area starts below the tab strip (34) and toolbar (42).
        top = 76

        page_keys(m, r, "page", win, popup=False, top=top)
        find_in_page(m, r, win, top)
        library_panel(m, r, win, top)

        # Shell keys while the page has focus: the page must not see them.
        m.click(win, 300, top + 300)
        n0 = (state(m) or {}).get("n")
        m.key("ctrl+t", delay=1.0)
        r.check("shell", "Ctrl+T opens a tab from the page", wait_for(lambda: any(t == 2 for t, _, _ in m.events())), "no tab 2 events")
        target = PAGE.as_uri() + "?second"
        m.type(target)
        m.key("Return", delay=1.5)
        r.check("shell", "the new tab's address bar takes typing", wait_for(lambda: state(m, tab=2)), "tab 2 never loaded")
        m.key("ctrl+Tab", delay=0.8)
        r.check("shell", "Ctrl+Tab switches tabs", "?second" not in m.xdo("getwindowname", win) and m.xdo("getwindowname", win).startswith("M{"), m.xdo("getwindowname", win))
        r.check("page", "page never saw Ctrl+T / Ctrl+Tab", (state(m, tab=1) or {}).get("n") == n0, f"{n0} vs {(state(m, tab=1) or {}).get('n')}")
        m.click(win, 300, top + 300)
        m.key("ctrl+l", delay=0.6)
        m.type("what is xvfb")
        m.key("Return", delay=1.0)
        urls = [a for t, name, a in m.events() if name == "UrlChanged" and t == 1]
        r.check("shell", "Ctrl+L, then a phrase searches", any("duckduckgo.com/?q=what+is+xvfb" in (u or "") for u in urls), str(urls[-3:]))
        m.key("ctrl+w", delay=1.0)
        tab2 = (state(m, tab=2) or {}).get("id", "?")
        r.check("shell", "Ctrl+W closes the tab (tab 2 shows again)", f'"id":"{tab2}"' in m.xdo("getwindowname", win), m.xdo("getwindowname", win))
        error_page(m, r, win, top)
        if wm:
            w0 = (state(m, tab=2) or {}).get("w", 0)
            m.click(win, 300, top + 300)
            m.key("F11", delay=1.5)
            w1 = (state(m, tab=2) or {}).get("w", 0)
            r.check("shell", "F11 toggles full screen", w1 > w0, f"{w0} -> {w1}")
            m.key("F11", delay=1.5)
            w2 = (state(m, tab=2) or {}).get("w", 0)
            r.check("shell", "F11 again leaves full screen", w2 < w1, f"{w1} -> {w2}")

        # Popups: open one from the page and run the same page keys in it.
        m.click(win, 500, top + 18)
        pwin = wait_for(lambda: m.window(r"^P\{"), 10)
        r.check("popup", "window.open makes a popup window", pwin)
        if pwin:
            m.xdo("windowactivate", "--sync", pwin)
            page_keys(m, r, "popup", pwin, popup=True, top=0)
            seen = [a for t, n, a in m.events() if n == "PopupTitleChanged"]
            r.check("popup", "popup state reached the shell", seen)
            m.key("ctrl+w", delay=1.0)
            r.check("popup", "Ctrl+W closes the popup", not m.window(r"^P\{"), "still open")
        profiles_panel(m, r, win, top)
        w0, h0 = settings_panel(m, r, win, top)
        m.stop()
        m = Morph(args.bin, display, home)
        settings_relaunch(m, r, w0, h0)
        single_instance(m, r, args.bin)
        bridge_round_trip(m, r, args.bin)
        # GPUI draws the frame itself only where the window manager supports
        # _GTK_FRAME_EXTENTS and a compositor runs; xfwm4 has both.
        if wm and shutil.which("xfwm4"):
            m.stop()
            wm.terminate()
            wm.wait(5)
            wm = subprocess.Popen(["xfwm4", "--compositor=on", "--replace"], env=dict(os.environ, DISPLAY=display), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(3.0)
            m = Morph(args.bin, display, home)
            frameless(m, r)
        # Identity on a home of its own (an identity locks the library at
        # the next launch, which would change the windows checked above).
        m.stop()
        m = Morph(args.bin, display, tempfile.mkdtemp(prefix="morph-xvfb-identity-"))
        r.check("identity", "a fresh home opens", wait_for(lambda: state(m), 20))
        user_id, backup = identity_panel(m, r)
        if user_id:
            identity_restore(r, args.bin, display, user_id, backup)
            identity_link(m, r, args.bin, display, user_id)
            m.stop()
            m = Morph(args.bin, display, str(m.home))
            identity_relaunch(m, r)
    finally:
        m.stop()
        if wm:
            wm.terminate()
        xvfb.terminate()
    return finish(r, args)


def finish(r, args):
    if args.report:
        Path(args.report).write_text(r.markdown() + "\n")
    failed = [row for row in r.rows if not row[2]]
    print(f"\n{len(r.rows) - len(failed)} passed, {len(failed)} failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
