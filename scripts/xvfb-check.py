#!/usr/bin/env python3
"""Drive the desktop shell (`morph`) in Xvfb with xdotool and check that keys
reach pages and popups, that the shell's own keys work, that typing a
phrase searches, that find in page works beside the page, that the library
panel (bookmarks, history) opens beside the page and takes the keyboard, and
that a failed load shows Morph's error page.

    scripts/xvfb-check.py [--bin PATH] [--report FILE] [--no-wm]
    scripts/xvfb-check.py --wayland [--scale 2] [--bin PATH] [--report FILE]

With --wayland the same checks run on native Wayland: a headless sway
(software rendering, no input devices) with morph-wl-input (built with the
workspace) as keyboard and pointer, and windows from `swaymsg -t get_tree`.

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
    def __init__(self, binary, display, home, extra_env=None):
        self.display = display
        self.home = Path(home)
        self.env = self.session_env(display)
        self.env.update(MORPH_HOME=home, MORPH_LOG_EVENTS="1", **(extra_env or {}))
        # A desktop of its own for the default-browser check: morph.desktop
        # and a `morph` on PATH (xdg-mime wants both), fresh config.
        xdg = Path(home) / "xdg"
        (xdg / "data" / "applications").mkdir(parents=True, exist_ok=True)
        (xdg / "config").mkdir(parents=True, exist_ok=True)
        (xdg / "bin").mkdir(parents=True, exist_ok=True)
        shutil.copy(ROOT / "packaging" / "linux" / "morph.desktop", xdg / "data" / "applications" / "morph.desktop")
        if not (xdg / "bin" / "morph").exists():
            (xdg / "bin" / "morph").symlink_to(Path(binary).resolve())
        # xdg-mime can take seconds on a real desktop; for Morph it takes
        # two here, so a window that waited for it would miss keys (the
        # checks' own calls, without MORPH_HOME, are not slowed).
        real = shutil.which("xdg-mime", path=os.environ.get("PATH"))
        if real and not (xdg / "bin" / "xdg-mime").exists():
            (xdg / "bin" / "xdg-mime").write_text(f'#!/bin/sh\n[ -n "$MORPH_HOME" ] && sleep 2\nexec {real} "$@"\n')
            (xdg / "bin" / "xdg-mime").chmod(0o755)
        self.env.update(XDG_DATA_HOME=str(xdg / "data"), XDG_CONFIG_HOME=str(xdg / "config"), PATH=f"{xdg / 'bin'}:{os.environ.get('PATH', '')}")
        for var in ("BROWSER", "XDG_CURRENT_DESKTOP", "DESKTOP_SESSION"):
            self.env.pop(var, None)
        self.log_path = Path(home) / "morph.log"
        self.log = open(self.log_path, "w")
        self.proc = subprocess.Popen([binary, PAGE.as_uri()], env=self.env, stdout=self.log, stderr=subprocess.STDOUT)
        self.read_at = 0

    @staticmethod
    def session_env(display):
        env = dict(os.environ, DISPLAY=display)
        env.pop("WAYLAND_DISPLAY", None)
        env.pop("MORPH_BACKEND", None)
        return env

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

    def move(self, win, x, y):
        self.xdo("mousemove", "--window", win, str(x), str(y))

    def click(self, win, x, y):
        self.move(win, x, y)
        time.sleep(0.15)
        self.xdo("click", "1")
        time.sleep(0.5)

    def activate(self, win):
        self.xdo("windowactivate", "--sync", win)

    def title(self, win):
        return self.xdo("getwindowname", win)

    def stop(self):
        self.proc.terminate()
        try:
            self.proc.wait(5)
        except subprocess.TimeoutExpired:
            self.proc.kill()


# evdev key codes (US layout) for the xdotool key names the checks use, and
# the characters typed, as (code, shift).
EVDEV = {
    "Escape": 1, "BackSpace": 14, "Tab": 15, "Return": 28, "space": 57, "Num_Lock": 69,
    "Home": 102, "Up": 103, "Prior": 104, "Left": 105, "Right": 106, "End": 107, "Down": 108, "Next": 109,
    "Insert": 110, "Delete": 111, "comma": 51, "period": 52, "slash": 53, "minus": 12,
    "KP_7": 71, "KP_8": 72, "KP_9": 73, "KP_Subtract": 74, "KP_4": 75, "KP_5": 76, "KP_6": 77, "KP_Add": 78,
    "KP_1": 79, "KP_2": 80, "KP_3": 81, "KP_0": 82, "KP_Decimal": 83, "KP_Enter": 96, "KP_Divide": 98, "KP_Multiply": 55,
    "F11": 87, "F12": 88, "ctrl": 29, "shift": 42, "alt": 56, "super": 125,
}
EVDEV.update({f"F{i}": 58 + i for i in range(1, 11)})
EVDEV.update({c: code for c, code in zip("1234567890", range(2, 12))})
for row, first in (("qwertyuiop", 16), ("asdfghjkl", 30), ("zxcvbnm", 44)):
    EVDEV.update({c: first + i for i, c in enumerate(row)})
CHARS = {c: (EVDEV[c], False) for c in "abcdefghijklmnopqrstuvwxyz1234567890"}
CHARS.update({c.upper(): (EVDEV[c], True) for c in "abcdefghijklmnopqrstuvwxyz"})
CHARS.update({" ": (57, False), "-": (12, False), "=": (13, False), "[": (26, False), "]": (27, False), ";": (39, False),
              "'": (40, False), "`": (41, False), "\\": (43, False), ",": (51, False), ".": (52, False), "/": (53, False)})
CHARS.update({c: (CHARS[b][0], True) for c, b in zip("!@#$%^&*()_+{}:\"~|<>?", "1234567890-=[];'`\\,./")})


class WaylandMorph(Morph):
    """The same Morph driven on a headless sway: keys and pointer from
    morph-wl-input (a virtual keyboard with the real evdev keymap, so keypad
    keys are keypad keys), windows from sway's tree."""

    input = None  # the shared morph-wl-input process
    layout = (1600, 1000)

    @staticmethod
    def session_env(display):
        env = dict(os.environ, WAYLAND_DISPLAY=display)
        for var in ("DISPLAY", "MORPH_BACKEND", "GDK_BACKEND"):
            env.pop(var, None)
        return env

    @classmethod
    def send(cls, command):
        cls.input.stdin.write(command + "\n")
        cls.input.stdin.flush()
        answer = cls.input.stdout.readline().strip()
        if answer != "ok":
            raise RuntimeError(f"morph-wl-input: {command}: {answer}")

    def press(self, code, down):
        self.send(f"key {code} {1 if down else 0}")

    def key(self, *keys, delay=0.35):
        for k in keys:
            *mods, name = k.split("+")
            codes = [EVDEV[mod.lower()] for mod in mods] + [EVDEV[name] if name in EVDEV else EVDEV[name.lower()]]
            for code in codes:
                self.press(code, True)
            for code in reversed(codes):
                self.press(code, False)
            time.sleep(delay)

    def type(self, text):
        for ch in text:
            code, shift = CHARS[ch]
            if shift:
                self.press(42, True)
            self.press(code, True)
            self.press(code, False)
            if shift:
                self.press(42, False)
            time.sleep(0.06)
        time.sleep(0.5)

    def tree(self):
        out = subprocess.run(["swaymsg", "-t", "get_tree"], env=self.env, capture_output=True, text=True).stdout
        found = []

        def walk(node):
            if node.get("pid") and node.get("visible", True):
                found.append(node)
            for child in node.get("nodes", []) + node.get("floating_nodes", []):
                walk(child)

        walk(json.loads(out or "{}"))
        return found

    def node(self, win):
        return next((n for n in self.tree() if str(n["id"]) == str(win)), None)

    def window(self, name):
        rx = re.compile(name)
        ids = [str(n["id"]) for n in self.tree() if rx.search(n.get("name") or "")]
        return ids[0] if ids else None

    def own_window(self):
        ids = [str(n["id"]) for n in self.tree() if n["pid"] == self.proc.pid]
        return ids[-1] if ids else None

    def geometry(self, win):
        r = (self.node(win) or {}).get("rect", {})
        return r.get("x", 0), r.get("y", 0), r.get("width", 0), r.get("height", 0)

    def move(self, win, x, y):
        wx, wy, _, _ = self.geometry(win)
        self.send(f"move {wx + x} {wy + y} {self.layout[0]} {self.layout[1]}")

    def click(self, win, x, y):
        self.move(win, x, y)
        time.sleep(0.15)
        self.send("button 272 1")
        self.send("button 272 0")
        time.sleep(0.5)

    def activate(self, win):
        subprocess.run(["swaymsg", f"[con_id={win}]", "focus"], env=self.env, capture_output=True)
        time.sleep(0.3)

    def title(self, win):
        return (self.node(win) or {}).get("name") or ""

    def ime(self, text):
        """Commit `text` as an input method would."""
        self.send(f"ime {text}")
        time.sleep(0.5)


def start_sway(input_bin, scale):
    """A headless sway (no input devices, software rendering) with windows
    floating at their own size and no borders, plus morph-wl-input as its
    keyboard and pointer. Returns (sway, socket name)."""
    runtime = Path(tempfile.mkdtemp(prefix="morph-wayland-"))
    runtime.chmod(0o700)
    config = runtime / "sway.conf"
    config.write_text(
        f"output HEADLESS-1 resolution 1600x1000 scale {scale}\n"
        "xwayland disable\n"
        "default_border none\ndefault_floating_border none\n"
        "for_window [app_id=\".*\"] floating enable\n"
        "focus_follows_mouse no\n"
        f"exec env > {runtime}/env\n"
    )
    env = dict(os.environ, XDG_RUNTIME_DIR=str(runtime), WLR_BACKENDS="headless", WLR_LIBINPUT_NO_DEVICES="1", WLR_RENDERER="pixman")
    for var in ("DISPLAY", "WAYLAND_DISPLAY", "SWAYSOCK"):
        env.pop(var, None)
    sway = subprocess.Popen(["sway", "-c", str(config)], env=env, stdout=open(runtime / "sway.log", "w"), stderr=subprocess.STDOUT)
    wait_for(lambda: (runtime / "env").exists() and "WAYLAND_DISPLAY=" in (runtime / "env").read_text(), 15)
    session = dict(line.split("=", 1) for line in (runtime / "env").read_text().splitlines() if "=" in line)
    os.environ.update(XDG_RUNTIME_DIR=str(runtime), SWAYSOCK=session.get("SWAYSOCK", ""))
    display = session["WAYLAND_DISPLAY"]
    WaylandMorph.layout = (int(1600 / scale), int(1000 / scale))
    WaylandMorph.input = subprocess.Popen([input_bin], env=dict(os.environ, WAYLAND_DISPLAY=display), stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    ready = WaylandMorph.input.stdout.readline().strip()
    if ready != "ready":
        raise RuntimeError(f"morph-wl-input did not start: {ready!r}")
    return sway, display


Driver = Morph


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
        m.move(win, 300, 50)  # over the shell's toolbar
        m.type("!")
        r.check(area, "typing with the pointer over the shell", (st() or {}).get("a") == "hello world!", str(st()))
        m.move(win, 300, top + 300)
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
    m.key("Tab", "Tab", "Tab")
    r.check(area, "Tab reaches a password field", (st() or {}).get("f") == "pw", str(st()))
    m.type("secret1")
    r.check(area, "typing lands in the password field", (st() or {}).get("pw") == "secret1" and (st() or {}).get("b") == "ello worl", str(st()))
    m.key("shift+Tab", "shift+Tab", "shift+Tab")
    r.check(area, "Shift+Tab leaves the password field", (st() or {}).get("f") == "a", str(st()))
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


def wayland_page(m, r, win, top, scale):
    """Native Wayland only: an input method's text reaches the page (as
    text, not keys), and the page renders at the output's scale."""
    st = lambda: state(m, tab=1) or {}  # noqa: E731
    m.click(win, 60, top + 18)
    m.key("End")
    before, n0 = st().get("a", ""), st().get("n")
    try:
        m.ime("日本語")
        ok, detail = wait_for(lambda: st().get("a") == before + "日本語", 3), str(st())
    except RuntimeError as e:
        ok, detail = False, str(e)
    r.check("wayland", "an input method's text reaches the page", ok, detail)
    r.check("wayland", "as text, not as keys", st().get("n") == n0, f"{n0} vs {st().get('n')}")
    r.check("wayland", f"the page renders at the output's scale ({scale:g})", st().get("d") == scale, str(st().get("d")))


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
    r.check("library", "the star fills and a Bookmarked toast shows at once", wait_for(lambda: m.lines(r"^morph-event drawn star=on toast=Bookmarked$"), 3), str(m.lines(r"^morph-event drawn")[-3:]))
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
    r.check("library", "the star empties with a Bookmark removed toast", wait_for(lambda: m.lines(r"^morph-event drawn star=off toast=Bookmark removed$"), 3), str(m.lines(r"^morph-event drawn")[-3:]))


def catalog(m):
    try:
        return json.loads((m.home / "profiles.json").read_text())
    except (OSError, json.JSONDecodeError):
        return {}


def profiles_panel(m, r, win, top):
    """Ctrl+Shift+M opens the profiles panel ready for a new name; Enter
    creates the profile and switches the window to it (a fresh blank tab
    whose address bar takes typing); Ctrl+Shift+M again switches back."""
    m.activate(win)
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
    r.check("profile", "the new profile starts with a blank tab", wait_for(lambda: m.title(win) == "Morph"), m.title(win))
    before = len(m.events())
    m.type(PAGE.as_uri() + "?work")
    m.key("Return", delay=1.5)
    loaded = wait_for(lambda: any(n == "TitleChanged" and "M{" in (a or "") for _, n, a in m.events()[before:]))
    r.check("profile", "its address bar takes typing and loads pages", loaded, m.title(win))
    data = m.home / "profiles" / (new[0] if new else "?") / "webkit" / "data"
    r.check("profile", "its web data lives in its own directory", wait_for(lambda: data.exists()), str(data))


def settings_file(m):
    cur = catalog(m).get("current", "default")
    path = m.home / "profiles" / cur / "settings.json"
    return path.read_text(errors="replace") if path.exists() else ""


def internal_pages(m, r, win, top):
    """morph://about typed in the address opens the About page in the tab;
    Back returns to the page."""
    m.activate(win)
    m.click(win, 300, top + 300)
    before = m.title(win)
    m.key("ctrl+l", delay=0.5)
    m.type("morph://about")
    m.key("Return", delay=1.0)
    r.check("about", "morph://about opens the About page", wait_for(lambda: m.title(win).startswith("About Morph")), m.title(win))
    m.key("alt+Left", delay=1.0)
    r.check("about", "Alt+Left goes back to the page", wait_for(lambda: not m.title(win).startswith("About")), f"{m.title(win)} (was {before})")


def settings_panel(m, r, win, top):
    """Ctrl+, opens the preferences as a page (a tab of its own) with the
    keyboard: / searches, Enter goes to the first setting, Right changes
    it. The tab position moves to the left (saved for the profile) and,
    once the settings tab closes, the page has the keyboard again."""
    m.activate(win)
    m.click(win, 300, top + 300)
    s0 = state(m) or {}
    w0, h0 = s0.get("w", 0), s0.get("h", 0)
    m.key("ctrl+comma", delay=0.8)
    r.check("settings", "Ctrl+, opens the preferences as a page", wait_for(lambda: m.title(win).startswith("Settings")), m.title(win))
    if shutil.which("xdg-mime"):
        env = {k: v for k, v in m.env.items() if k != "MORPH_HOME"}
        get = lambda: ",".join(subprocess.run(["xdg-mime", "query", "default", t], env=env, capture_output=True, text=True).stdout.strip() for t in ("x-scheme-handler/http", "x-scheme-handler/https"))  # noqa: E731
        m.key("slash", delay=0.4)
        m.type("default browser")
        m.key("Return", delay=0.4)
        m.key("Return", delay=0.3)
        settings_check = time.time()
    m.key("slash", delay=0.4)
    m.type("tab position")
    m.key("Return", delay=0.4)
    m.key("Right", delay=0.4)
    m.key("Right", delay=0.8)
    r.check("settings", "searching, Enter and Right twice move the tabs to the left (saved)", wait_for(lambda: '"left"' in settings_file(m)), settings_file(m))
    if shutil.which("xdg-mime"):
        # Making Morph the default runs off the UI thread: the keys above did not wait.
        r.check("settings", "the keys did not wait for the default-browser work", time.time() - settings_check < 8.0, f"{time.time() - settings_check:.1f}s")
        r.check("settings", "Enter on Default browser makes Morph the default (xdg-mime reads it back)", wait_for(lambda: get() == "morph.desktop,morph.desktop", 15), get())
    m.key("ctrl+w", delay=1.0)
    side = wait_for(lambda: (state(m) or {}).get("w") == w0 - 220 and (state(m) or {}).get("h") == h0 + 34 and w0 - 220)
    r.check("settings", "closing the settings tab shows the page beside a 220 px tab list", side, f"{w0}x{h0} -> {state(m)}")
    r.check("settings", "after closing, the page has the keyboard", page_takes_key(m, "z", "KeyZ"), str(state(m)))
    return w0, h0


def bar_keys(m, win, top, tabs, *keys):
    """Reach the tab bar's buttons from the keyboard: Ctrl+L, then Tab once
    for the position button, twice for collapse."""
    m.click(win, 300, top + 300) if win else None
    m.key("ctrl+l", delay=0.6)
    for _ in range(tabs):
        m.key("Tab", delay=0.3)
    for k in keys:
        m.key(k, delay=0.4)


def tab_bar(m, r, win, top):
    """The tab bar's own buttons: the position menu (Top/Bottom/Left/Right)
    and the collapse button, in all four positions, from the keyboard.
    Ends with the tabs on top, expanded."""
    m.activate(win)
    m.click(win, 300, top + 300)
    s0 = state(m) or {}
    w0, h0 = s0.get("w", 0), s0.get("h", 0)
    sizes = {
        "left": ((w0 - 220, h0 + 34), (w0 - 48, h0 + 34)),
        "right": ((w0 - 220, h0 + 34), (w0 - 48, h0 + 34)),
        "bottom": ((w0, h0), (w0, h0)),
        "top": ((w0, h0), (w0, h0)),
    }
    downs = {"top": 1, "bottom": 2, "left": 3, "right": 4}
    # The first Ctrl+L after a click in the page can race the page's
    # keyboard handoff under Xvfb: one round trip first.
    bar_keys(m, win, top, 0, "Escape")
    for position in ("left", "right", "bottom", "top"):
        # The page area moved: click where the page is in every layout.
        page_top = top if position == "top" else 42
        bar_keys(m, win, page_top, 1, "Return", *(["Down"] * downs[position]), "Return")
        expanded, collapsed = sizes[position]
        size = lambda: ((state(m) or {}).get("w"), (state(m) or {}).get("h"))  # noqa: E731
        ok = wait_for(lambda: f'"{position}"' in settings_file(m)) and wait_for(lambda: size() == expanded)
        r.check("tabs", f"the position button's menu moves the tabs: {position}", ok, f"{size()} vs {expanded} {settings_file(m)}")
        bar_keys(m, win, page_top, 2, "Return")
        ok = wait_for(lambda: '"sidebar_collapsed":true' in settings_file(m).replace(" ", "")) and wait_for(lambda: size() == collapsed)
        r.check("tabs", f"the collapse button collapses the tabs ({position}, saved)", ok, f"{size()} vs {collapsed} {settings_file(m)}")
        bar_keys(m, win, page_top, 2, "Return")
        ok = wait_for(lambda: '"sidebar_collapsed":false' in settings_file(m).replace(" ", "")) and wait_for(lambda: size() == expanded)
        r.check("tabs", f"and expands them again ({position})", ok, f"{size()} vs {expanded}")
    m.click(win, 300, top + 300)
    r.check("tabs", "after the buttons, the page has the keyboard", page_takes_key(m, "z", "KeyZ"), str(state(m)))


def serve_tests():
    """Serve tests/xvfb over http on localhost (icons are read from web
    pages only). Returns the base URL."""
    import functools
    import http.server
    import threading

    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=str(ROOT / "tests" / "xvfb"))
    handler.log_message = lambda *a, **k: None
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return f"http://127.0.0.1:{server.server_address[1]}/"


def favicon(m, r, binary, base):
    """A page's icon (link rel=icon) reaches the shell for its tab."""
    before = len(m.events())
    subprocess.run([binary, base + "icon.html"], env=m.env, capture_output=True, timeout=15)
    got = wait_for(lambda: [a for _, n, a in m.events()[before:] if n == "Favicon"], 10)
    r.check("icons", "a page's icon reaches its tab", got and "bytes" in got[-1], str(got))


def hand_to(r, group, binary, url, env):
    """Launch `binary url` and expect it to hand the URL to the running
    Morph and exit; a launch that keeps running is a failed check, not a
    crashed run."""
    try:
        subprocess.run([binary, url], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15)
        return True
    except subprocess.TimeoutExpired:
        r.check(group, "a second launch hands its URL over and exits", False, "still running after 15 s")
        return False


def file_picker(r, binary, display, base):
    """A page's <input type=file> opens the system's file dialog (answered
    from MORPH_TEST_PICK_OPEN here) and the page gets the file."""
    pick = Path(tempfile.mkdtemp(prefix="morph-xvfb-upload-")) / "hello.txt"
    pick.write_text("hello from the file dialog\n")
    m2 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-files-"), extra_env={"MORPH_TEST_PICK_OPEN": str(pick)})
    try:
        # Its own window: the first Morph still runs on this display.
        win = wait_for(lambda: m2.own_window(), 20)
        r.check("files", "a Morph for the upload page opens", win)
        if not win:
            return
        before = len(m2.events())
        if not hand_to(r, "files", binary, base + "upload.html", m2.env):
            return

        def uploaded():
            for _, n, a in reversed(m2.events()[before:]):
                if n == "TitleChanged" and a and '"U{' in a:
                    return json.loads(json.loads(a)[1:])
            return None

        wait_for(lambda: any(n == "TitleChanged" and a and "upload" in a for _, n, a in m2.events()[before:]), 10)
        win = m2.window("upload") or win
        m2.activate(win)
        m2.click(win, 400, 76 + 200)
        asked = wait_for(lambda: [n for _, n, _ in m2.events()[before:] if n == "FileChooser"], 6)
        r.check("files", "a click on a file input asks the shell for files", asked, str([n for _, n, _ in m2.events()[before:]][-6:]))
        got = wait_for(uploaded, 8) or {}
        r.check("files", "the page gets the file the dialog chose", got.get("n") == 1 and got.get("name") == "hello.txt" and got.get("text") == "hello from the file dialog", str(got))
    finally:
        m2.stop()


def select_menu(r, binary, display, base):
    """A click on a <select> opens its list; Down and Enter pick the next
    option and the page sees one change (Linux: GTK's list on X11, the
    shell's own on Wayland)."""
    m2 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-select-"))
    try:
        # Its own window: the first Morph still runs on this display.
        win = wait_for(lambda: m2.own_window(), 20)
        r.check("select", "a Morph for the select page opens", win)
        if not win:
            return
        before = len(m2.events())
        if not hand_to(r, "select", binary, base + "select.html", m2.env):
            return

        def sel():
            for _, n, a in reversed(m2.events()[before:]):
                if n == "TitleChanged" and a and '"S{' in a:
                    return json.loads(json.loads(a)[1:])
            return None

        s0 = wait_for(sel, 10) or {}
        r.check("select", "the select page loads", s0.get("fruit") == "banana", str(s0))
        if not s0:
            return
        time.sleep(1)
        win = m2.window("S\\{") or win
        m2.activate(win)
        m2.click(win, s0["x"] + 20, 76 + s0["y"] + 8)
        time.sleep(1.5)
        m2.key("Down", delay=0.4)
        m2.key("Return", delay=1.0)
        got = wait_for(lambda: (sel() or {}).get("fruit") == "cherry" and sel()) or sel() or {}
        r.check("select", "Down and Enter in its list pick the next option", got.get("fruit") == "cherry" and got.get("changes") == 1, str(got))
    finally:
        m2.stop()


def last_tab_quits(r, binary, display):
    """Closing the last tab quits Morph (exit code 0) after saving."""
    m2 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-quit-"))
    try:
        # Its own window: the first Morph still runs on this display.
        wait_for(lambda: state(m2), 20)
        win = wait_for(lambda: m2.own_window(), 20)
        r.check("quit", "a Morph with one tab opens", win and wait_for(lambda: state(m2), 20))
        if not win:
            return
        m2.activate(win)
        m2.click(win, 300, 76 + 300)
        m2.key("ctrl+w", delay=0.5)
        code = wait_for(lambda: m2.proc.poll() is not None and str(m2.proc.poll()), 8)
        r.check("quit", "closing the last tab quits with exit code 0", code == "0", f"exit {m2.proc.poll()}")
        r.check("quit", "and it said so (layout saved first)", m2.lines(r"^morph-event quit"), "no quit event")
    finally:
        m2.stop()


def settings_relaunch(m, r, w0, h0):
    """The profile's settings apply at start; Ctrl+, then Up puts the tabs
    back on top."""
    win = wait_for(lambda: m.window("Morph"), 20)
    r.check("settings", "relaunch opens the window", win)
    ok = wait_for(lambda: (state(m) or {}).get("w") == w0 - 220 and (state(m) or {}).get("h") == h0 + 34, 20)
    r.check("settings", "the saved left tab list applies at start", ok, str(state(m)))
    if not win:
        return
    m.activate(win)
    m.click(win, 600, 300)
    m.key("ctrl+comma", delay=0.8)
    m.key("slash", delay=0.4)
    m.type("tab position")
    m.key("Return", delay=0.4)
    m.key("Left", delay=0.4)
    m.key("Left", delay=0.8)
    m.key("ctrl+w", delay=1.0)
    back = wait_for(lambda: (state(m) or {}).get("w") == w0 and (state(m) or {}).get("h") == h0)
    r.check("settings", "Left twice puts the tabs back on top", back and '"top"' in settings_file(m), f"{state(m)} {settings_file(m)}")


def frameless(m, r):
    """With a compositor running, the window is frameless: no WM frame, a
    thin resize border, the tab row drags the window and a double click on
    it maximizes; the page keeps the keyboard; the close button closes."""
    win = wait_for(lambda: m.window("Morph"), 20)
    r.check("frame", "window opens with a compositor", win)
    if not win or not wait_for(lambda: state(m), 20):
        r.check("frame", "page loads", False)
        return
    hints = m.title(win) and subprocess.run(["xprop", "-id", win, "_MOTIF_WM_HINTS"], env=m.env, capture_output=True, text=True).stdout
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
    m.activate(win)
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
    (password, Tab, Shift+Tab, Tab, password again), c copies the recovery
    words (the clipboard clears itself), d saves them, Enter goes on, s
    makes shards and a saves them all, b saves a backup, Escape closes the
    panel and the page has the keyboard again. File dialogs answer from
    MORPH_TEST_PICK_DIR."""
    win, w0 = open_identity(m, r, "identity")
    m.key("1", delay=0.8)
    m.type(PASSWORD)
    m.key("Tab", "shift+Tab", "Tab", delay=0.3)
    m.type(PASSWORD)
    m.key("Return", delay=0.8)
    made = wait_for(lambda: identity_file(m).get("user_id"))
    r.check("identity", "1, a password, Tab, Shift+Tab, Tab, the password again: the profile has an identity", made, str(identity_file(m)))
    pick = Path(m.env["MORPH_TEST_PICK_DIR"])
    m.key("c", delay=0.8)
    r.check("identity", "c copies the recovery words", wait_for(lambda: m.lines(r"morph-event identity copied words")))
    r.check("identity", "the clipboard clears itself", wait_for(lambda: m.lines(r"morph-event identity clipboard cleared"), 10))
    m.key("d", delay=1.0)
    words_file = pick / "morph-recovery-words-default.txt"
    r.check("identity", "d saves the words where the Save dialog says", wait_for(lambda: words_file.exists()), str(list(pick.iterdir())))
    words = words_file.read_text().splitlines() if words_file.exists() else []
    r.check("identity", "the file holds the 24 words on a line", any(len(line.split()) == 24 for line in words), str(len(words)))
    r.check("identity", "only its owner can read it", words_file.exists() and (words_file.stat().st_mode & 0o777) == 0o600)
    m.key("Return", delay=0.8)
    m.key("s", delay=0.8)
    fill(m, PASSWORD)
    m.key("a", delay=1.0)
    shards = lambda: sorted(pick.glob("morph-shard-default-*-of-5.txt"))  # noqa: E731
    r.check("identity", "s makes shards; a saves all five, one file each", wait_for(lambda: len(shards()) == 5), str(shards()))
    r.check("identity", "each says which shard it is and how many it needs", all("Any 3 of the 5" in p.read_text() for p in shards()) and shards() != [])
    log = "\n".join(m.lines(r"."))
    r.check("identity", "no secret reaches the log", not any(w in log for w in words[3:4]) and not any(p.read_text().split()[-1] in log for p in shards()))
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
    return made, backup, shards()


def identity_shards_restore(r, binary, display, user_id, shards):
    """Another home restores from three of the five shard files: 3, the
    files (Choose files… answers from MORPH_TEST_PICK_OPEN), no backup
    password, a new password."""
    picked = ";".join(str(p) for p in shards[1:4])
    m4 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-shards-"), {"MORPH_TEST_PICK_OPEN": picked})
    try:
        r.check("shards", "a Morph with another home opens", wait_for(lambda: state(m4), 20))
        open_identity(m4, r, "shards")
        m4.key("3", delay=0.8)
        fill(m4, picked.replace(";", "; "), "", PASSWORD)
        got = wait_for(lambda: identity_file(m4, "default").get("user_id"))
        r.check("shards", "three shard files restore the same identity", got and got == user_id, f"{got} vs {user_id}")
    finally:
        m4.stop()


def identity_relaunch(m, r):
    """With an identity the library is encrypted; without a keyring (as
    under Xvfb) the next launch opens the library asking for the password.
    The password unlocks it; Escape closes it; the page has the keyboard."""
    win = wait_for(m.own_window, 20)
    r.check("identity", "after a relaunch the page loads", wait_for(lambda: state(m), 20))
    w = (state(m) or {}).get("w", 0)
    m.activate(win)
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
    m2 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-restore-"))
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
    m3 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-link-"))
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
        m3.activate(new_win)
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
    m.activate(win)
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


def popup_close(m, r, binary):
    """A sign-in style popup that posts to its opener and calls
    window.close() closes, and the tab's popup count goes back down."""
    before = len(m.events())
    page = (ROOT / "tests" / "xvfb" / "opener.html").as_uri()
    subprocess.run([binary, page], env=m.env, capture_output=True, timeout=15)

    def opener():
        for _, n, a in reversed(m.events()[before:]):
            if n == "TitleChanged" and a and '"O{' in a:
                return json.loads(json.loads(a)[1:])
        return None

    r.check("popup", "the opener page loads in a new tab", wait_for(opener, 10), "no opener title")
    win = wait_for(m.own_window, 10)
    m.activate(win)
    m.click(win, 400, 76 + 300)
    r.check("popup", "a click opens the sign-in popup", wait_for(lambda: (opener() or {}).get("opened"), 5), str(opener()))
    got = wait_for(lambda: (opener() or {}).get("closed"), 10)
    r.check("popup", "the popup's message reached the opener", (opener() or {}).get("msg") == "signed-in", str(opener()))
    r.check("popup", "window.close() closes the popup", got, str(opener()))
    names = [n for _, n, _ in m.events()[before:]]
    r.check("popup", "and the tab's popup count goes back down", names.count("PopupOpened") >= 1 and names.count("PopupOpened") == names.count("PopupClosed"), str([n for n in names if n.startswith("Popup")]))
    r.check("popup", "no popup window is left", wait_for(lambda: m.window(r"^Signing in") is None, 3), "still open")


def media(m, r, binary):
    """Bundled clips (VP8 WebM, H.264 MP4) play: muted autoplay, and
    currentTime keeps moving."""
    before = len(m.events())
    page = (ROOT / "tests" / "xvfb" / "video.html").as_uri()
    subprocess.run([binary, page], env=m.env, capture_output=True, timeout=15)

    def clips():
        for _, n, a in reversed(m.events()[before:]):
            if n == "TitleChanged" and a and '"V{' in a:
                return json.loads(json.loads(a)[1:])
        return None

    wait_for(lambda: clips() and all(c["r"] >= 2 for c in clips().values()), 10)
    first = clips() or {}
    time.sleep(1.5)
    second = clips() or {}
    for kind in ("webm", "mp4"):
        a, b = first.get(kind, {}), second.get(kind, {})
        ok = a.get("e") == 0 and b.get("e") == 0 and b.get("r", 0) >= 2 and a.get("t") != b.get("t") and not b.get("p", True)
        r.check("media", f"a {kind} clip plays (currentTime moves)", ok, f"{a} -> {b}")


def gdk_scale_ignored(m, r, binary, display):
    """GDK_SCALE and GDK_DPI_SCALE (set by some desktops for GTK apps) do not
    make pages larger than the shell: a Morph started with them settles on
    the pixel ratio (on X11 also the page size) the running one shows (both are read once
    they settle; the first report can come before GTK knows the output's
    scale, which the next frame corrects)."""
    def size(s):
        s = s or {}
        return s.get("w"), s.get("h"), s.get("d")

    m2 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-gdkscale-"), extra_env={"GDK_SCALE": "2", "GDK_DPI_SCALE": "1.5"})
    try:
        # The device pixel ratio says it: GDK_SCALE=2 doubles it. Sizes are
        # only compared on X11: a tiling compositor (sway) halves the first
        # window when the second opens, and the first page's report of its
        # new size can lag (two flaky runs on 2026-11-15).
        def settled():
            a, b = size(state(m2)), size(state(m))
            return state(m2) and a[2] == b[2] and (Driver is WaylandMorph or a == b)
        ok = wait_for(settled, 20)
        r.check("scale", "with GDK_SCALE=2 the page matches the shell's scale", ok, f"{state(m2)} vs {state(m)}")
    finally:
        m2.stop()

def render_check(r, binary, display):
    """The start-up render check (crates/morph-desktop/src/render_check.rs).
    The child side reports what real GPUs hit: an off-screen window cannot
    make a GL context (WebKit aborted on that with a GPU buffer), and a test
    page paints where the shell's pages live. A Morph with the check forced
    (no GPU here, so it would not run by itself) starts and shows its page;
    on Wayland it hands off-screen pages shared-memory buffers."""
    env = Driver.session_env(display)
    wayland = "WAYLAND_DISPLAY" in env

    def child(where, what, extra=None):
        e = dict(env, GDK_BACKEND="wayland" if wayland else "x11", **(extra or {}))
        try:
            return subprocess.run([binary, "--probe-render", where, what], env=e, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30).returncode
        except subprocess.TimeoutExpired:
            return "timeout"

    code = child("offscreen", "gl")
    r.check("render", "an off-screen window reports no GL context", code == 20, f"exit {code}")
    if wayland:
        code = child("offscreen", "page", {"WEBKIT_DMABUF_RENDERER_FORCE_SHM": "1"})
        r.check("render", "an off-screen test page paints with shared-memory buffers", code == 0, f"exit {code}")
    else:
        code = child("window", "page")
        r.check("render", "a test page in an off-screen-placed window paints", code == 0, f"exit {code}")
    m2 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-render-"), extra_env={"MORPH_DMABUF_PROBE": "1"})
    try:
        s = wait_for(lambda: state(m2), 30)
        notes = m2.lines(r"^morph: ")
        if wayland:
            ok = s and any("WEBKIT_DMABUF_RENDERER_FORCE_SHM=1" in n for n in notes)
            name = "with the render check forced, pages show on shared memory"
        else:
            ok = s and not notes
            name = "with the render check forced, pages show and nothing changes"
        r.check("render", name, ok, f"{s} {notes}")
    finally:
        m2.stop()


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
    r.check("error", "Alt+Left leaves the error page for the page under it", back, f"{m.title(win)} {state(m, tab=2)}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=str(Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "morph"))
    ap.add_argument("--report")
    ap.add_argument("--no-wm", action="store_true")
    ap.add_argument("--wayland", action="store_true", help="run on a headless sway (native Wayland) instead of Xvfb")
    ap.add_argument("--scale", type=float, default=1.0, help="the Wayland output's scale")
    ap.add_argument("--input-bin", help="morph-wl-input (default: next to --bin)")
    args = ap.parse_args()

    global Driver
    xvfb, wm = None, None
    if args.wayland:
        Driver = WaylandMorph
        xvfb, display = start_sway(args.input_bin or str(Path(args.bin).with_name("morph-wl-input")), args.scale)
        wm = xvfb  # sway manages windows (full screen) itself
    else:
        display = ":" + str(70 + os.getpid() % 200)
        xvfb = subprocess.Popen(["Xvfb", display, "-screen", "0", "1600x1000x24", "-ac", "-noreset"], stderr=subprocess.DEVNULL)
        time.sleep(1.5)
        if not args.no_wm and shutil.which("openbox"):
            wm = subprocess.Popen(["openbox"], env=dict(os.environ, DISPLAY=display), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(1.0)
    home = tempfile.mkdtemp(prefix="morph-xvfb-")
    m = Driver(args.bin, display, home)
    r = Results()
    try:
        win = wait_for(lambda: m.window("Morph"), 20)
        r.check("shell", "window opens", win)
        r.check("shell", "page loads", wait_for(lambda: state(m), 20))
        # Looked up again: while Morph starts, the page's own GTK window can
        # carry the name for a moment (seen when the render check runs).
        win = m.window("Morph") or win
        if not win:
            return finish(r, args)
        # The page area starts below the tab strip (34) and toolbar (42).
        top = 76

        page_keys(m, r, "page", win, popup=False, top=top)
        if args.wayland:
            wayland_page(m, r, win, top, args.scale)
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
        r.check("shell", "Ctrl+Tab switches tabs", "?second" not in m.title(win) and m.title(win).startswith("M{"), m.title(win))
        r.check("page", "page never saw Ctrl+T / Ctrl+Tab", (state(m, tab=1) or {}).get("n") == n0, f"{n0} vs {(state(m, tab=1) or {}).get('n')}")
        m.click(win, 300, top + 300)
        m.key("ctrl+l", delay=0.6)
        m.type("what is xvfb")
        m.key("Return", delay=1.0)
        urls = [a for t, name, a in m.events() if name == "UrlChanged" and t == 1]
        r.check("shell", "Ctrl+L, then a phrase searches (Google by default)", any("google.com/search?q=what+is+xvfb" in (u or "") for u in urls), str(urls[-3:]))
        m.key("ctrl+w", delay=1.0)
        tab2 = (state(m, tab=2) or {}).get("id", "?")
        r.check("shell", "Ctrl+W closes the tab (tab 2 shows again)", f'"id":"{tab2}"' in m.title(win), m.title(win))
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
            m.activate(pwin)
            page_keys(m, r, "popup", pwin, popup=True, top=0)
            seen = [a for t, n, a in m.events() if n == "PopupTitleChanged"]
            r.check("popup", "popup state reached the shell", seen)
            m.key("ctrl+w", delay=1.0)
            r.check("popup", "Ctrl+W closes the popup", not m.window(r"^P\{"), "still open")
        profiles_panel(m, r, win, top)
        tab_bar(m, r, win, top)
        internal_pages(m, r, win, top)
        w0, h0 = settings_panel(m, r, win, top)
        m.stop()
        m = Driver(args.bin, display, home)
        settings_relaunch(m, r, w0, h0)
        single_instance(m, r, args.bin)
        bridge_round_trip(m, r, args.bin)
        popup_close(m, r, args.bin)
        media(m, r, args.bin)
        gdk_scale_ignored(m, r, args.bin, display)
        render_check(r, args.bin, display)
        base = serve_tests()
        favicon(m, r, args.bin, base)
        file_picker(r, args.bin, display, base)
        select_menu(r, args.bin, display, base)
        last_tab_quits(r, args.bin, display)
        # GPUI draws the frame itself only where the window manager supports
        # _GTK_FRAME_EXTENTS and a compositor runs; xfwm4 has both.
        if wm and not args.wayland and shutil.which("xfwm4"):
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
        pick = tempfile.mkdtemp(prefix="morph-xvfb-pick-")
        m = Driver(args.bin, display, tempfile.mkdtemp(prefix="morph-xvfb-identity-"), {"MORPH_TEST_PICK_DIR": pick, "MORPH_TEST_CLIPBOARD_SECS": "2"})
        r.check("identity", "a fresh home opens", wait_for(lambda: state(m), 20))
        user_id, backup, shards = identity_panel(m, r)
        if user_id:
            identity_restore(r, args.bin, display, user_id, backup)
            if len(shards) == 5:
                identity_shards_restore(r, args.bin, display, user_id, shards)
            identity_link(m, r, args.bin, display, user_id)
            m.stop()
            m = Driver(args.bin, display, str(m.home))
            identity_relaunch(m, r)
    finally:
        m.stop()
        if wm and wm is not xvfb:
            wm.terminate()
        if WaylandMorph.input:
            WaylandMorph.input.terminate()
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
