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
LINKS = ROOT / "tests" / "xvfb" / "links.html"
EVENT = re.compile(r'^morph-event tab=(\d+) (\w+)(?:\((.*)\))?')
# With a relay of our own (--local-relay): the Relays check and the new
# device's link screen use it (MORPH_LINK_RELAYS, a dead address first, so
# the others have to carry the connection) and fetch no relay info. Its
# address is private, so codes carry no circuit through it (only relays on
# routable addresses go in codes) and wait 5 s for one; the other homes
# keep the built-in relay.
LINK_ENV = {}
LOCAL_RELAY = False
# With --app-server as well: a Weirdnet app (morph-services' serve-app
# example) behind the local relay, its peer id once it holds a slot.
APP_PEER = None


class Morph:
    def __init__(self, binary, display, home, extra_env=None, args=None):
        self.display = display
        self.home = Path(home)
        self.env = self.session_env(display)
        # One window with the page, whatever the last run left open (the
        # windows check turns restoring back on).
        self.env.update(MORPH_HOME=str(home), MORPH_LOG_EVENTS="1", MORPH_NO_RESTORE="1")
        self.env.update(extra_env or {})
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
        self.proc = subprocess.Popen([binary, *([PAGE.as_uri()] if args is None else args)], env=self.env, stdout=self.log, stderr=subprocess.STDOUT)
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
        ids = self.windows()
        return ids[-1] if ids else None

    def windows(self):
        """This process's visible windows."""
        return self.xdo("search", "--onlyvisible", "--pid", str(self.proc.pid)).split()

    def click_with(self, win, x, y, mod=None, button=1):
        """Click with `button` (3: right) while holding `mod` (shift, ctrl)."""
        self.move(win, x, y)
        time.sleep(0.15)
        if mod:
            self.xdo("keydown", mod)
        self.xdo("click", str(button))
        if mod:
            self.xdo("keyup", mod)
        time.sleep(0.5)

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
        ids = self.windows()
        return ids[-1] if ids else None

    def windows(self):
        return [str(n["id"]) for n in self.tree() if n["pid"] == self.proc.pid]

    def click_with(self, win, x, y, mod=None, button=1):
        code = 272 if button == 1 else 273
        self.move(win, x, y)
        time.sleep(0.15)
        if mod:
            self.press(EVDEV[mod], True)
        self.send(f"button {code} 1")
        self.send(f"button {code} 0")
        if mod:
            self.press(EVDEV[mod], False)
        time.sleep(0.5)

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


def focus_info(m):
    """Who has the keyboard, for failure details: the X focus window (name
    and pid) and the shell's last few events."""
    out = ""
    if not isinstance(m, WaylandMorph):
        win = m.xdo("getwindowfocus")
        out = f"x focus {win} {m.xdo('getwindowname', win)!r} pid {m.xdo('getwindowpid', win)}; "
    return out + "events " + str([n for _, n, _ in m.events()[-6:]])


def wait_for(fn, timeout=6.0):
    end = time.time() + timeout
    value = fn()
    while not value and time.time() < end:
        time.sleep(0.2)
        value = fn()
    return value


def settled_layout(morph, win, fallback, timeout=10.0, still=1.0):
    """The window width and the page state once neither has changed for
    `still` seconds (or the last seen after `timeout`)."""
    end = time.time() + timeout
    last, since = None, time.time()
    width, page = morph.geometry(win)[2], state(morph) or fallback
    while time.time() < end:
        width, page = morph.geometry(win)[2], state(morph) or fallback
        now = (width, page.get("w"), page.get("h"))
        if now != last:
            last, since = now, time.time()
        elif time.time() - since >= still:
            break
        time.sleep(0.2)
    return width, page


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
    # The tabs the window showed stay saved with the profile it left (a
    # switch used to rewrite its tabs.json without them, so they were lost).
    left = m.home / "profiles" / "default" / "tabs.json"
    kept = wait_for(lambda: left.exists() and PAGE.name in left.read_text(errors="replace"))
    r.check("profile", "the profile it left keeps its tabs", kept, left.read_text(errors="replace")[:300] if left.exists() else "no tabs.json")
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


def relays_section(m, r):
    """Settings → Relays reads the real state: nothing runs at first (it
    says so, never "connected"), and Enter on it starts a check whose node
    reports a real state (connected and reserved need the relay reachable,
    so any state passes; the log says which)."""
    rx = r"morph-event relays nodes=0 checking=false state=no-node count=1"
    r.check("settings", "the Relays section lists the built-in relay as not connected while nothing runs", wait_for(lambda: m.lines(rx)), str(m.lines(r"morph-event relays")[-3:]))
    m.key("slash", delay=0.4)
    m.type("relays")
    m.key("Return", delay=0.6)
    m.key("Return", delay=1.0)
    started = wait_for(lambda: m.lines(r"morph-event relays nodes=1 checking=true state=(not-connected|connected|reserved)"), 15)
    r.check("settings", "Enter on Relays starts a check: a real node reports the relay's state", started, str(m.lines(r"morph-event relays")[-3:]))
    if LOCAL_RELAY:
        # The local relay's first address goes nowhere: the slot comes over
        # another one, and the check measures a round trip to it.
        reserved = wait_for(lambda: m.lines(r"morph-event relays nodes=1 checking=true state=reserved count=1 rtt=\d+ addrs=3"), 20)
        r.check("settings", "the check reserves a slot on the local relay over its second address and shows the round trip", reserved, str(m.lines(r"morph-event relays")[-3:]))
    else:
        time.sleep(3.0)
    last = (m.lines(r"morph-event relays nodes=") or ["none"])[-1]
    print(f"INFO  relays  after the check started: {last}", flush=True)


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
    # After the timed default-browser checks: the relay check waits on its own.
    relays_section(m, r)
    m.key("ctrl+w", delay=1.0)
    side = wait_for(lambda: (state(m) or {}).get("w") == w0 - 220 and (state(m) or {}).get("h") == h0 + 34 and w0 - 220)
    r.check("settings", "closing the settings tab shows the page beside a 220 px tab list", side, f"{w0}x{h0} -> {state(m)}")
    r.check("settings", "after closing, the page has the keyboard", page_takes_key(m, "z", "KeyZ"), f"{state(m)} {focus_info(m)}")
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
    r.check("tabs", "after the buttons, the page has the keyboard", page_takes_key(m, "z", "KeyZ"), f"{state(m)} {focus_info(m)}")


def serve_tests():
    """Serve tests/xvfb over http on localhost (icons are read from web
    pages only). Returns the base URL."""
    import functools
    import http.server
    import threading

    class Handler(http.server.SimpleHTTPRequestHandler):
        """Files, plus a download like a GitHub release asset: /dl/asset
        redirects to /dl/object, an attachment with a suggested name."""

        def do_GET(self):
            if self.path.startswith("/ping"):
                self.send_response(200)
                self.send_header("Content-Type", "text/plain")
                self.send_header("Content-Length", "4")
                self.end_headers()
                self.wfile.write(b"pong")
            elif self.path.startswith("/dl/asset"):
                self.send_response(302)
                self.send_header("Location", "/dl/object?token=x")
                self.end_headers()
            elif self.path.startswith("/dl/slow"):
                # A few seconds long, honouring Range (pause and resume).
                rng = self.headers.get("Range")
                SLOW_RANGES.append(rng)
                start = int(rng.split("=", 1)[1].split("-", 1)[0]) if rng and rng.startswith("bytes=") else 0
                self.send_response(206 if rng else 200)
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("Content-Disposition", f'attachment; filename="{SLOW_NAME}"')
                self.send_header("Content-Length", str(len(SLOW_BYTES) - start))
                if rng:
                    self.send_header("Content-Range", f"bytes {start}-{len(SLOW_BYTES) - 1}/{len(SLOW_BYTES)}")
                self.end_headers()
                try:
                    for i in range(start, len(SLOW_BYTES), 32 * 1024):
                        self.wfile.write(SLOW_BYTES[i:i + 32 * 1024])
                        self.wfile.flush()
                        time.sleep(0.12)
                except (BrokenPipeError, ConnectionResetError):
                    pass
            elif self.path.startswith("/jwt-download.html"):
                # A page signed in with JWTs in its storage (see jwt_page).
                body = jwt_page("store=0" not in self.path).encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            elif self.path.startswith("/dl/jwt"):
                # Behind a JavaScript login: a resume (Range) needs the page's
                # live JWT as a bearer token; the page's own link needs none.
                rng = self.headers.get("Range")
                auth = self.headers.get("Authorization")
                JWT_REQUESTS.append((rng, auth))
                if rng and auth != f"Bearer {JWT_GOOD}":
                    self.send_response(401)
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                start = int(rng.split("=", 1)[1].split("-", 1)[0]) if rng and rng.startswith("bytes=") else 0
                self.send_response(206 if rng else 200)
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("Content-Disposition", f'attachment; filename="{JWT_NAME}"')
                self.send_header("Content-Length", str(len(SLOW_BYTES) - start))
                if rng:
                    self.send_header("Content-Range", f"bytes {start}-{len(SLOW_BYTES) - 1}/{len(SLOW_BYTES)}")
                self.end_headers()
                try:
                    for i in range(start, len(SLOW_BYTES), 32 * 1024):
                        self.wfile.write(SLOW_BYTES[i:i + 32 * 1024])
                        self.wfile.flush()
                        time.sleep(0.12)
                except (BrokenPipeError, ConnectionResetError):
                    pass
            elif self.path.startswith("/dl/object"):
                self.send_response(200)
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("Content-Disposition", f'attachment; filename="{DOWNLOAD_NAME}"')
                self.send_header("Content-Length", str(len(DOWNLOAD_BYTES)))
                self.end_headers()
                # In pieces, so the shell sees progress.
                for i in range(0, len(DOWNLOAD_BYTES), 64 * 1024):
                    self.wfile.write(DOWNLOAD_BYTES[i:i + 64 * 1024])
                    self.wfile.flush()
                    time.sleep(0.05)
            else:
                super().do_GET()

        def log_message(self, *a, **k):
            pass

    handler = functools.partial(Handler, directory=str(ROOT / "tests" / "xvfb"))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return f"http://127.0.0.1:{server.server_address[1]}/"


def frame_stats(m, since=0):
    """The `morph-frames` lines (MORPH_FRAME_STATS=1) after line `since`,
    as dicts."""
    out = []
    for line in m.lines(r"^morph-frames ")[since:]:
        out.append(dict(kv.split("=", 1) for kv in line.split()[1:] if "=" in kv))
    return out


def busy_page(r, binary, display, base):
    """A page like Gmail at rest (busy.html: a request and a timer every
    100 ms, an endless animation) keeps its scripts on time, with the page
    alone and with the identity panel's link QR open beside it (Compose
    in Gmail stalled for 20 s on alien-blue, RTX 3060 at scale 2: the web
    process spent its time reading GPU frames back into memory). Frames are
    copied at most 60 times a second, and none while a page is still."""
    g = "busy page"
    home = tempfile.mkdtemp(prefix="morph-xvfb-busy-")
    m = Driver(binary, display, home, {"MORPH_FRAME_STATS": "1"})
    try:
        r.check(g, "the still page loads", wait_for(lambda: state(m), 20))
        time.sleep(2.5)
        n = len(frame_stats(m))
        time.sleep(4.5)
        still = frame_stats(m, n)
        # Only the off-screen (Wayland) backend copies frames; X11 pages are
        # windows of their own.
        offscreen = bool(frame_stats(m))
        if offscreen:
            r.check(g, "a still page copies no frames", still and max(float(s["copied_fps"]) for s in still) < 2, str(still))
        before = len(m.events())
        subprocess.run([binary, base + "busy.html"], env=m.env, capture_output=True, timeout=15)
        got = wait_for(lambda: (state(m) or {}).get("nf", 0) > 20, 20)
        r.check(g, "busy.html runs", got, str(state(m)))
        time.sleep(3)
        n = len(frame_stats(m))
        time.sleep(5)
        s = state(m) or {}
        busy = frame_stats(m, n)
        detail = f"page {s}; frames {busy}"
        print(f"busy page alone: {detail}")
        r.check(g, "the page counts itself visible", s.get("v") == "visible", detail)
        r.check(g, "its 100 ms timer is not throttled (90% within 250 ms)", 0 < s.get("t", -1) < 250, detail)
        r.check(g, "its requests stay quick (90% within 250 ms)", 0 <= s.get("f", -1) < 250, detail)
        if offscreen:
            r.check(g, "frames are copied at most 60 times a second", busy and max(float(b["copied_fps"]) for b in busy) <= 62, detail)
        # The identity panel with a new device's link QR (4), beside it.
        win = wait_for(m.own_window, 10)
        m.activate(win)
        m.click(win, 300, 76 + 300)
        m.key("ctrl+shift+l", delay=1.5)
        m.key("4", delay=0.5)
        offer = wait_for(lambda: m.lines(r"morph-event link offer="), 15)
        r.check(g, "the link QR shows", offer, "no offer logged")
        n0 = s.get("nt", 0)
        time.sleep(5)
        s = state(m) or {}
        detail = f"page {s}; frames {frame_stats(m)[-2:]}"
        print(f"busy page with the link panel: {detail}")
        r.check(g, "with the link panel open the timer keeps time", 0 < s.get("t", -1) < 250 and s.get("nt", 0) - n0 > 30, detail)
        r.check(g, "with the link panel open requests stay quick", 0 <= s.get("f", -1) < 250, detail)
        encoded = m.lines(r"morph-event identity qr encoded")
        r.check(g, "the QR is encoded once, not every frame", 1 <= len(encoded) <= 2, f"{len(encoded)} encodes")
    finally:
        m.stop()


def hover_page(r, binary, display, base):
    """A big page with a dark <select> whose classes change as the pointer
    moves (hover.html, like Gmail): Morph's select-theming script leaves it
    alone at rest (it rewrote the select 240 times a second, every frame,
    forever) and while the pointer sweeps, and the page's timer keeps time."""
    g = "hover page"
    m = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-hover-"), {"MORPH_FRAME_STATS": "1"})
    try:
        r.check(g, "the page loads", wait_for(lambda: state(m), 20))
        subprocess.run([binary, base + "hover.html"], env=m.env, capture_output=True, timeout=15)
        r.check(g, "hover.html runs", wait_for(lambda: "w" in (state(m) or {}), 20), str(state(m)))
        time.sleep(3)
        s = state(m) or {}
        r.check(g, "at rest nothing rewrites the select", s.get("w", -1) == 0, str(s))
        win = wait_for(m.own_window, 10)
        n = len(m.lines(r"^morph-render "))
        end = time.time() + 5
        i = 0
        while time.time() < end:
            m.move(win, 200 + (i * 7) % 700, 200 + (i * 13) % 500)
            i += 1
        s = state(m) or {}
        renders = m.lines(r"^morph-render ")[n:]
        detail = f"page {s}; {i} moves sent; {renders[-2:]}"
        print(f"hover sweep: {detail}")
        r.check(g, "the page saw the pointer move", s.get("mv", 0) > 20, detail)
        r.check(g, "while the pointer sweeps the select is rewritten at most a few times", 0 <= s.get("w", -1) <= 4, detail)
        r.check(g, "while the pointer sweeps the page's 100 ms timer keeps time", 0 < s.get("t", -1) < 250, detail)
    finally:
        m.stop()


def long_address(r, binary, display, base):
    """A long address is clipped where the bar ends, before the profile
    chip and the buttons beside it (it ran under the chip: 'activity:75
    m3talsmith 948225/'), and scrolls to the caret while it is edited."""
    g = "address bar"
    url = base + "keys.html?" + "&".join(f"activity{i}={i * 7919}" for i in range(60))
    m = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-url-"), {"MORPH_LOG_LAYOUT": "1"})
    try:
        r.check(g, "a long address loads", wait_for(lambda: state(m), 20))
        subprocess.run([binary, url], env=m.env, capture_output=True, timeout=15)
        time.sleep(2.5)

        def last():
            rows = m.lines(r"^morph-event address text_w=")
            return dict(kv.split("=", 1) for kv in rows[-1].split()[2:]) if rows else {}

        a = wait_for(lambda: (lambda d: d if float(d.get("text_w", 0)) > float(d.get("bar_w", 1)) else None)(last()), 10) or last()
        detail = str(a)
        r.check(g, "the address is longer than the bar (the check means something)", a and float(a["text_w"]) > float(a["bar_w"]), detail)
        r.check(g, "the bar ends before the profile chip", a and 0 < float(a["right"]) <= float(a["chip_left"]), detail)
        r.check(g, "unfocused, the address shows from its start", a and float(a.get("scroll", 1)) == 0, detail)
        m.key("ctrl+l", delay=0.6)
        m.key("End", delay=0.8)
        b = last()
        r.check(g, "editing at its end scrolls the text to the caret", b and float(b.get("scroll", 0)) > 0, str(b))
        m.key("Home", delay=0.8)
        c = last()
        r.check(g, "Home scrolls back to its start", c and float(c.get("scroll", 1)) == 0, str(c))
        m.key("Escape", delay=0.4)
    finally:
        m.stop()


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


DOWNLOAD_NAME = "morph-test-asset.tar.gz"
DOWNLOAD_BYTES = bytes((i * 7) % 251 for i in range(1_200_000))


def download(r, binary, display, base):
    """A click on a link that redirects to an attachment (a GitHub release
    asset) asks where to save it with the suggested name (answered from
    MORPH_TEST_PICK_DIR), and the file lands there whole, with progress on
    the way and no staging file left behind."""
    home = Path(tempfile.mkdtemp(prefix="morph-xvfb-download-"))
    pick = home / "picked"
    staging = home / "Downloads"
    pick.mkdir()
    staging.mkdir()
    # The downloads folder the bytes are staged in (glib reads it here).
    (home / "xdg" / "config").mkdir(parents=True, exist_ok=True)
    (home / "xdg" / "config" / "user-dirs.dirs").write_text(f'XDG_DOWNLOAD_DIR="{staging}"\n')
    m2 = Driver(binary, display, str(home), extra_env={"MORPH_TEST_PICK_DIR": str(pick)})
    try:
        win = wait_for(lambda: m2.own_window(), 20)
        r.check("download", "a Morph for the download page opens", win)
        if not win:
            return
        before = len(m2.events())
        if not hand_to(r, "download", binary, base + "download.html", m2.env):
            return
        wait_for(lambda: any(n == "TitleChanged" and a and "download" in a for _, n, a in m2.events()[before:]), 10)
        win = m2.window("download") or win
        m2.activate(win)
        m2.click(win, 400, 76 + 200)
        target = pick / DOWNLOAD_NAME
        saved = wait_for(lambda: m2.lines("morph-event download saved"), 30)
        events = [a or "" for _, n, a in m2.events()[before:] if n == "Download"]
        started = [a for a in events if "Started" in a]
        r.check("download", "a redirect to an attachment starts a download with the suggested name", started and DOWNLOAD_NAME in started[0], str(events[:3]) or str(m2.lines("morph-event")[-6:]))
        r.check("download", "the save dialog's answer gets the whole file", saved and target.exists() and target.read_bytes() == DOWNLOAD_BYTES, f"{saved} {sorted(p.name for p in pick.iterdir())} {target.stat().st_size if target.exists() else 0} bytes")
        r.check("download", "no staging file is left behind", not list(staging.iterdir()), str(list(staging.iterdir())))
        if sys.platform.startswith("linux"):
            r.check("download", "progress reaches the shell", any("Progress" in a for a in events), str(events[-3:]))
    finally:
        m2.stop()


SLOW_NAME = "morph-slow-asset.bin"
SLOW_BYTES = bytes((i * 13) % 251 for i in range(1_600_000))
SLOW_RANGES = []
JWT_NAME = "morph-jwt-asset.bin"
JWT_REQUESTS = []


def make_jwt(exp_from_now, sub):
    import base64

    def enc(b):
        return base64.urlsafe_b64encode(b).rstrip(b"=").decode()

    header = json.dumps({"alg": "HS256", "typ": "JWT"}).encode()
    claims = json.dumps({"sub": sub, "exp": int(time.time()) + exp_from_now}).encode()
    return f"{enc(header)}.{enc(claims)}.{enc(b'signature')}"


JWT_GOOD = make_jwt(3600, "me")
JWT_OLD = make_jwt(-600, "me")


def jwt_page(live):
    """A page signed in the JavaScript way: an expired JWT in localStorage
    and (when `live`) the live one inside a JSON blob in sessionStorage,
    and a link filling the page to the download behind the login."""
    store = f"sessionStorage.setItem('auth', JSON.stringify({{user: 'me', access_token: '{JWT_GOOD}'}}));" if live else ""
    return f"""<!doctype html><html><head><meta charset="utf-8"><title>jwt download</title>
<style>body {{ margin: 0; }} a {{ position: fixed; inset: 0; display: block; font-size: 120px; }}</style>
<script>localStorage.setItem('token', '{JWT_OLD}'); {store}</script></head>
<body><a id="a" href="/dl/jwt">jwt download</a></body></html>"""


def morph_rows(m, key):
    """The newest `morph-event <key> ...` line as a dict of its fields."""
    rows = m.lines(rf"^morph-event {re.escape(key)} ")
    if not rows:
        return {}
    rest = rows[-1][len(f"morph-event {key} "):]
    return dict(kv.split("=", 1) for kv in rest.split() if "=" in kv)


def downloads_panel(r, binary, display, base, shots=None):
    """The toolbar's downloads button sits just left of ☰; Ctrl+Shift+Y
    opens the panel under it with a tile per download: a progress bar and
    time left while it runs, a click to open it when done ("Opens in …"),
    pause and resume (the rest fetched with a Range request) whose buttons
    don't count as a click on the tile, and once saved no bar, no estimate,
    and the file opened (logged only: MORPH_TEST_NO_OPEN)."""
    g = "downloads"
    home = Path(tempfile.mkdtemp(prefix="morph-xvfb-downloads-"))
    pick = home / "picked"
    staging = home / "Downloads"
    pick.mkdir()
    staging.mkdir()
    (home / "xdg" / "config").mkdir(parents=True, exist_ok=True)
    (home / "xdg" / "config" / "user-dirs.dirs").write_text(f'XDG_DOWNLOAD_DIR="{staging}"\n')
    env = {"MORPH_TEST_PICK_DIR": str(pick), "MORPH_LOG_LAYOUT": "1", "MORPH_TEST_NO_OPEN": "1"}
    m2 = Driver(binary, display, str(home), extra_env=env)

    def shot(name):
        if shots and not isinstance(m2, WaylandMorph) and shutil.which("import"):
            Path(shots).mkdir(parents=True, exist_ok=True)
            subprocess.run(["import", "-window", "root", str(Path(shots) / f"{name}.png")], env=m2.env, capture_output=True)

    def center(name):
        h = morph_rows(m2, f"downloads hit name={name}")
        return (float(h["x"]) + float(h["w"]) / 2, float(h["y"]) + float(h["h"]) / 2) if h else None

    try:
        win = wait_for(lambda: m2.own_window(), 20)
        r.check(g, "a Morph for the downloads page opens", win)
        if not win:
            return
        bar = wait_for(lambda: morph_rows(m2, "toolbar"), 10) or {}
        ok = bar and float(bar["downloads_right"]) <= float(bar["menu_left"]) <= float(bar["downloads_right"]) + 8
        r.check(g, "the downloads button sits just left of the ☰ button", ok, str(bar))
        before = len(m2.events())
        if not hand_to(r, g, binary, base + "slow-download.html", m2.env):
            return
        wait_for(lambda: any(n == "TitleChanged" and a and "slow download" in a for _, n, a in m2.events()[before:]), 10)
        win = m2.window("slow download") or win
        m2.activate(win)
        m2.click(win, 400, 76 + 200)
        started = wait_for(lambda: [a for _, n, a in m2.events()[before:] if n == "Download" and a and "Started" in a], 15)
        r.check(g, "the slow download starts", started, str(m2.lines("morph-event")[-5:]))
        if not started:
            return
        m2.key("ctrl+shift+y", delay=0.8)
        r.check(g, "Ctrl+Shift+Y opens the downloads panel", wait_for(lambda: m2.lines(r"^morph-event downloads panel open"), 5), str(m2.lines("morph-event")[-5:]))
        key = next((ln.split()[3] for ln in reversed(m2.lines(r"^morph-event downloads row id=\d+ "))), "")
        dl = key.split("=", 1)[1] if "=" in key else "1"
        row = lambda: morph_rows(m2, f"downloads row id={dl}")  # noqa: E731
        running = wait_for(lambda: (lambda x: x if x.get("state") == "running" and x.get("eta", "").endswith("_left") else None)(row()), 8) or row()
        r.check(g, "a running download shows a progress bar and a time left", running.get("bar") == "determinate" and re.search(r"\d+[sm]_left$", running.get("eta", "")), str(running))
        shot("downloads-panel-running")
        tile = center(f"tile-{dl}")
        if tile:
            m2.click(win, int(tile[0]), int(tile[1]))
        waiting = wait_for(lambda: (lambda x: x if x.get("open", "-").startswith("Opens_") else None)(row()), 4) or row()
        r.check(g, "a click on a running tile opens it when done (Opens in …)", re.match(r"Opens_(in_\d+[sm]|when_done)", waiting.get("open", "")), str(waiting))
        shot("downloads-panel-opens-when-done")
        pause = center(f"pause-{dl}")
        if pause:
            m2.click(win, int(pause[0]), int(pause[1]))
        paused = wait_for(lambda: (lambda x: x if x.get("state") == "paused" else None)(row()), 4) or row()
        r.check(g, "Pause pauses it: Paused instead of a time left", paused.get("state") == "paused" and paused.get("eta") == "Paused", str(paused))
        r.check(g, "the Pause button is not a click on the tile", paused.get("open", "-").startswith("Opens_"), str(paused))
        received = (paused.get("status") or "")
        time.sleep(1.0)
        shot("downloads-panel-paused")
        resume = wait_for(lambda: center(f"resume-{dl}"), 3)
        if resume:
            m2.click(win, int(resume[0]), int(resume[1]))
        r.check(g, "Resume runs it again", wait_for(lambda: row().get("state") in ("running", "saved"), 4), str(row()))
        saved = wait_for(lambda: m2.lines("morph-event download saved"), 40)
        target = pick / SLOW_NAME
        r.check(g, "the resumed download lands whole", saved and target.exists() and target.read_bytes() == SLOW_BYTES, f"{saved} {target.stat().st_size if target.exists() else 0} bytes, paused at {received}")
        resumed = [x for x in SLOW_RANGES if x]
        r.check(g, "resume asked only for the rest (a Range request)", resumed and resumed[-1] != "bytes=0-", str(SLOW_RANGES))
        opened = wait_for(lambda: m2.lines(r"^morph-event download open "), 5)
        r.check(g, "the waiting tile opens the file once saved", opened and opened[-1].endswith(str(target)), str(opened))
        done = wait_for(lambda: (lambda x: x if x.get("state") == "saved" else None)(row()), 4) or row()
        r.check(g, "once saved the bar and the estimate are gone", done.get("bar") == "none" and done.get("eta") == "-" and done.get("open") == "-", str(done))
        shot("downloads-panel-done")
        m2.key("Escape", delay=0.6)
        r.check(g, "Escape closes the panel", wait_for(lambda: m2.lines(r"^morph-event downloads panel closed"), 4))
    finally:
        m2.stop()


def downloads_js_login(r, binary, display, base, live):
    """A download behind a JavaScript login: pause it, and the resume the
    server refuses (401) goes on with the page's live JWT read from its
    storage (`live`), the expired one skipped; with only the expired one,
    the page starts the download again and the tile takes it over."""
    g = "downloads-jwt" if live else "downloads-jwt-fallback"
    home = Path(tempfile.mkdtemp(prefix="morph-xvfb-jwt-"))
    pick = home / "picked"
    staging = home / "Downloads"
    pick.mkdir()
    staging.mkdir()
    (home / "xdg" / "config").mkdir(parents=True, exist_ok=True)
    (home / "xdg" / "config" / "user-dirs.dirs").write_text(f'XDG_DOWNLOAD_DIR="{staging}"\n')
    env = {"MORPH_TEST_PICK_DIR": str(pick), "MORPH_LOG_LAYOUT": "1", "MORPH_TEST_NO_OPEN": "1"}
    m2 = Driver(binary, display, str(home), extra_env=env)
    JWT_REQUESTS.clear()

    def center(name):
        h = morph_rows(m2, f"downloads hit name={name}")
        return (float(h["x"]) + float(h["w"]) / 2, float(h["y"]) + float(h["h"]) / 2) if h else None

    try:
        win = wait_for(lambda: m2.own_window(), 20)
        r.check(g, "a Morph for the signed-in page opens", win)
        if not win:
            return
        before = len(m2.events())
        if not hand_to(r, g, binary, base + "jwt-download.html" + ("" if live else "?store=0"), m2.env):
            return
        wait_for(lambda: any(n == "TitleChanged" and a and "jwt download" in a for _, n, a in m2.events()[before:]), 10)
        win = m2.window("jwt download") or win
        m2.activate(win)
        m2.click(win, 400, 76 + 200)
        started = wait_for(lambda: [a for _, n, a in m2.events()[before:] if n == "Download" and a and "Started" in a], 15)
        r.check(g, "the download behind the login starts", started, str(m2.lines("morph-event")[-5:]))
        if not started:
            return
        m2.key("ctrl+shift+y", delay=0.8)
        wait_for(lambda: m2.lines(r"^morph-event downloads panel open"), 5)
        key = next((ln.split()[3] for ln in reversed(m2.lines(r"^morph-event downloads row id=\d+ "))), "")
        dl = key.split("=", 1)[1] if "=" in key else "1"
        row = lambda: morph_rows(m2, f"downloads row id={dl}")  # noqa: E731
        wait_for(lambda: row().get("state") == "running", 8)
        pause = wait_for(lambda: center(f"pause-{dl}"), 4)
        if pause:
            m2.click(win, int(pause[0]), int(pause[1]))
        paused = wait_for(lambda: (lambda x: x if x.get("state") == "paused" else None)(row()), 4) or row()
        r.check(g, "it pauses", paused.get("state") == "paused", str(paused))
        time.sleep(1.0)
        resume = wait_for(lambda: center(f"resume-{dl}"), 3)
        if resume:
            m2.click(win, int(resume[0]), int(resume[1]))
        saved = wait_for(lambda: m2.lines("morph-event download saved"), 60)
        target = pick / JWT_NAME
        whole = bool(saved) and target.exists() and target.read_bytes() == SLOW_BYTES
        refused = [a for rng, a in JWT_REQUESTS if rng and a != f"Bearer {JWT_GOOD}"]
        r.check(g, "the first resume is refused without the page's token", refused, str([(rng, (a or "")[:12]) for rng, a in JWT_REQUESTS]))
        if live:
            found = m2.lines(r"^morph-event tab=0 DownloadToken\(\d+, found: true\)")
            r.check(g, "the live JWT is read from the page's storage", found, str(m2.lines("DownloadAuth|DownloadToken")))
            ranged = [a for rng, a in JWT_REQUESTS if rng and a == f"Bearer {JWT_GOOD}"]
            never_old = not any(a == f"Bearer {JWT_OLD}" for _, a in JWT_REQUESTS)
            r.check(g, "the resume goes on with it (Range + Bearer), never the expired one", ranged and never_old, str(len(ranged)))
        else:
            r.check(g, "no usable token: the page is asked to start it again", m2.lines(r"DownloadAuth\(Retrigger"), str(m2.lines("DownloadAuth|DownloadToken")))
            fresh = sum(1 for rng, _ in JWT_REQUESTS if not rng)
            r.check(g, "the page's own new download is taken over by the tile", fresh >= 2 and len(m2.lines(r"^morph-event downloads row id=")) > 0, str(fresh))
        r.check(g, "the download lands whole", whole, f"{saved} {target.stat().st_size if target.exists() else 0} bytes")
        leaked = any(JWT_GOOD in ln or JWT_OLD in ln for ln in m2.lines(""))
        r.check(g, "no token is ever logged", not leaked, "a JWT is in Morph's output" if leaked else "")
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


def page_clipboard(r, binary, display, base, wayland):
    """Paste, copy and cut in a page, as github.com's paste handler reads
    them (clipboardData types, items, files): alpha.9 crashed (SIGSEGV in
    WebKitGTK's clipboard code) on the first paste on native Wayland, where
    the shell now hands pages its own clipboard."""
    m2 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-paste-"))
    try:
        win = wait_for(lambda: m2.own_window(), 20)
        r.check("paste", "a Morph for the paste page opens", win)
        if not win:
            return
        before = len(m2.events())
        if not hand_to(r, "paste", binary, base + "paste.html", m2.env):
            return

        def st():
            for _, n, a in reversed(m2.events()[before:]):
                if n == "TitleChanged" and a and '"V{' in a:
                    return json.loads(json.loads(a)[1:])
            return None

        def settle(want):
            return wait_for(lambda: want(st() or {}) and st(), 5) or st() or {}

        alive = lambda: m2.proc.poll() is None
        r.check("paste", "the paste page loads", wait_for(st, 10), "no state")
        time.sleep(1)
        win = m2.window("V\\{") or win
        m2.activate(win)
        # The shell's own clipboard, from the address field.
        m2.key("ctrl+l", delay=0.5)
        m2.type("hello paste")
        m2.key("ctrl+a", delay=0.3)
        m2.key("ctrl+c", delay=0.5)
        m2.key("Escape", delay=0.8)
        m2.click(win, 100, 76 + 80)
        time.sleep(0.8)
        m2.key("ctrl+v", delay=0.5)
        got = settle(lambda s: s.get("pastes") == 1)
        r.check("paste", "Ctrl+V in a textarea: Morph keeps running", alive(), f"exit {m2.proc.poll()}")
        if not alive():
            return
        r.check(
            "paste",
            "the page's paste listener reads the clipboard (types, getData, items)",
            # WebKit's own paste on X11 lists no types (it reads lazily).
            got.get("pastes") == 1
            and got.get("text") == "hello paste"
            and (not wayland or ("text/plain" in got.get("types", []) and got.get("items", 0) >= 1)),
            str(got),
        )
        r.check("paste", "the text lands in the textarea", got.get("t") == "hello paste", str(got))
        m2.click(win, 100, 76 + 240)
        time.sleep(0.8)
        m2.key("shift+Insert", delay=0.5)
        got = settle(lambda s: s.get("e") == "hello paste")
        r.check("paste", "Shift+Insert pastes into a contenteditable", alive() and got.get("e") == "hello paste", str(got))
        # Copy from the page, paste back: the shell's clipboard has it.
        m2.click(win, 100, 76 + 80)
        time.sleep(0.5)
        m2.key("ctrl+a", delay=0.3)
        m2.type("from the page")
        m2.key("ctrl+a", delay=0.3)
        m2.key("ctrl+c", delay=0.8)
        got = settle(lambda s: s.get("copies") == 1)
        r.check("paste", "Ctrl+C in the page fires its copy event", alive() and got.get("copies") == 1, str(got))
        m2.click(win, 100, 76 + 240)
        time.sleep(0.5)
        m2.key("ctrl+a", delay=0.3)
        m2.key("ctrl+v", delay=0.5)
        got = settle(lambda s: s.get("e") == "from the page")
        r.check("paste", "what the page copied pastes back", got.get("e") == "from the page", str(got))
        m2.click(win, 100, 76 + 80)
        time.sleep(0.5)
        m2.key("ctrl+a", delay=0.3)
        m2.key("ctrl+x", delay=0.5)
        got = settle(lambda s: s.get("t") == "")
        r.check("paste", "Ctrl+X cuts the selection", alive() and got.get("t") == "", str(got))
        m2.key("ctrl+v", delay=0.5)
        got = settle(lambda s: s.get("t") == "from the page")
        r.check("paste", "and Ctrl+V puts it back", got.get("t") == "from the page", str(got))
        # Middle clicks (the primary selection on Linux), on text and not.
        for where, (x, y) in (("on editable text", (100, 76 + 240)), ("elsewhere", (600, 76 + 450)), ("on a link", (60, 76 + 332))):
            m2.move(win, x, y)
            time.sleep(0.5)
            if wayland:
                m2.send("button 274 1")
                m2.send("button 274 0")
            else:
                m2.xdo("click", "2")
            time.sleep(1.5)
            r.check("paste", f"a middle click {where}: Morph keeps running", alive(), f"exit {m2.proc.poll()}")
            if not alive():
                return
    finally:
        m2.stop()


MAIN_MENU = ["New tab", "New window", "Find in page", "Bookmarks", "History", "Downloads", "Identity", "Profiles", "Settings", "About Morph", "Quit"]
MENU_SEPARATED = {"Find in page", "Identity", "About Morph", "Quit"}


def main_menu(r, binary, display):
    """Every entry of the ☰ menu, clicked with the mouse in a fresh Morph,
    does what it says (the menu opened but its entries did nothing in
    c00f4fc)."""

    def row_center(label):
        y = 72 + 1 + 4
        for name in MAIN_MENU:
            if name in MENU_SEPARATED:
                y += 9
            if name == label:
                return y + 13
            y += 26
        raise KeyError(label)

    for label in MAIN_MENU:
        m2 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-menu-"))
        try:
            win = wait_for(lambda: m2.own_window(), 20)
            s0 = wait_for(lambda: state(m2), 20)
            if not (win and s0):
                r.check("menu", f"{label}: a Morph opens with its page", False, "no window or page")
                continue
            time.sleep(1.0)
            win = m2.own_window() or win
            m2.activate(win)
            # The first states can come before the window took its size: on
            # Wayland at scale 2 a fresh window reported a 395-wide page and
            # reached its 510 a moment later, after the fixed sleep, so the
            # menu click used the old width and missed. Measure, and take the
            # click positions, once the window and the page hold still.
            width, s0 = settled_layout(m2, win, s0)
            w0, h0 = s0.get("w", 0), s0.get("h", 0)
            before = len(m2.events())
            m2.click(win, width - 23, 55)
            time.sleep(0.8)
            m2.click(win, width - 248 + 80, row_center(label))
            time.sleep(1.0)
            st = lambda: state(m2) or {}  # noqa: E731
            if label == "New tab":
                ok = wait_for(lambda: any(t == 2 for t, _, _ in m2.events()[before:]))
                detail = "no tab 2"
            elif label == "New window":
                ok = wait_for(lambda: m2.lines(r"^morph-event window opened 2"))
                detail = str(m2.lines("morph-event window"))
            elif label == "Find in page":
                ok = wait_for(lambda: st().get("h", h0) < h0)
                detail = f"page height {h0} -> {st().get('h')}"
            elif label in ("Bookmarks", "History", "Identity", "Profiles"):
                ok = wait_for(lambda: st().get("w", w0) < w0)
                detail = f"page width {w0} -> {st().get('w')} (no panel beside it)"
            elif label == "Downloads":
                ok = wait_for(lambda: m2.lines(r"^morph-event downloads panel open"))
                detail = str(m2.lines("morph-event")[-4:])
            elif label == "Settings":
                ok = wait_for(lambda: "Settings" in m2.title(win) or "Preferences" in m2.title(win))
                detail = m2.title(win)
            elif label == "About Morph":
                ok = wait_for(lambda: m2.title(win).startswith("About Morph"))
                detail = m2.title(win)
            else:
                ok = wait_for(lambda: m2.proc.poll() == 0)
                detail = f"exit {m2.proc.poll()}"
            r.check("menu", f"☰ → {label} works", ok, detail)
        finally:
            m2.stop()


def luminance(px):
    return 0.299 * px[0] + 0.587 * px[1] + 0.114 * px[2]


def popup_backing(r, binary, display, shots=None):
    """Popups never have an opaque black backing: with the ☰ menu or the
    downloads panel open, the page is still drawn around the popup, and the
    pixels just outside its rounded bottom-left corner are the page's, not
    black (on X11 the page's window is hidden under a popup, which once left
    the whole page area black behind it). Needs Pillow."""
    try:
        from PIL import Image
    except ImportError:
        r.check("popups", "transparent popup backing (needs Pillow)", True, "skipped: no Pillow")
        return
    home = tempfile.mkdtemp(prefix="morph-xvfb-popups-")
    m2 = Driver(binary, display, home)
    png = Path(home) / "popup.png"

    def corner(name, width_from_edge, row):
        """Screenshot, then look for the popup's box on logical row `row`
        scanning right from the page's middle; true when the page is drawn
        beside it and the pixel outside its bottom-left corner is light."""
        screenshot(m2, png)
        img = Image.open(png).convert("RGB")
        win = m2.own_window()
        gx, gy, w, _ = m2.geometry(win)
        f = img.width / (WaylandMorph.layout[0] if isinstance(m2, WaylandMorph) else img.width)
        at = lambda x, y: img.getpixel((min(int((gx + x) * f), img.width - 1), min(int((gy + y) * f), img.height - 1)))  # noqa: E731
        beside = luminance(at(w // 2 - 100, 400))
        left = next((x for x in range(int(w * 0.42), w) if luminance(at(x, row)) < 120), None)
        if left is None:
            return False, f"no popup box found on row {row}; page beside {beside:.0f}"
        bottom = next((y for y in range(row, row + 800) if luminance(at(left + 6, y)) > 170), None)
        if bottom is None:
            return False, "no bottom edge"
        outside = luminance(at(left + 1, bottom - 1))
        # The page is white (keys.html) beside and under the popup.
        ok = beside > 200 and outside > 170
        if shots:
            Path(shots).mkdir(parents=True, exist_ok=True)
            shutil.copy(png, Path(shots) / f"popups-{name}.png")
        return ok, f"page beside {beside:.0f}, corner outside popup at ({left + 1},{bottom - 1}) {outside:.0f}"

    try:
        win = wait_for(lambda: m2.own_window(), 20)
        ok = win and wait_for(lambda: state(m2), 20)
        r.check("popups", "a Morph opens with its page", bool(ok), "no window or page")
        if not ok:
            return
        time.sleep(1.0)
        win = m2.own_window() or win
        m2.activate(win)
        _, _, width, _ = m2.geometry(win)
        m2.click(win, width - 23, 55)
        time.sleep(1.0)
        ok, detail = corner("menu", 248, 150)
        r.check("popups", "the ☰ menu floats over the page: no black backing at its corners", ok, detail)
        m2.key("Escape")
        time.sleep(0.6)
        m2.key("ctrl+shift+y")
        time.sleep(1.0)
        ok, detail = corner("downloads", 0, 110)
        r.check("popups", "the downloads panel floats over the page: no black backing", ok, detail)
    finally:
        m2.stop()


def suggestions_check(r, binary, display, shots=None):
    """Typing in the address bar drops a list of suggestions from history
    and bookmarks below the bar (with a "search for" row last); the top
    match completes inline as selected text that Right accepts (and that
    Right leaves alone as plain cursor movement when there is none); Down
    and Enter open a row, Escape closes the list, a click opens a row."""
    g = "omnibox"
    now = int(time.time() * 1000)
    entries = [
        {"url": "https://github.com/WeirdBrains/morph", "title": "WeirdBrains/morph", "visited_ms": now - 3_600_000},
        {"url": "https://github.com/WeirdBrains/morph", "title": "WeirdBrains/morph", "visited_ms": now - 7_200_000},
        {"url": "https://github.com/WeirdBrains/weirdnet", "title": "WeirdBrains/weirdnet", "visited_ms": now - 86_400_000},
        {"url": "https://gitlab.com/explore", "title": "Explore GitLab", "visited_ms": now - 40 * 86_400_000},
        {"url": "https://example.org/", "title": "Example Domain", "visited_ms": now - 5 * 86_400_000},
    ]

    def launch():
        home = Path(tempfile.mkdtemp(prefix="morph-xvfb-omnibox-"))
        prof = home / "profiles" / "default"
        prof.mkdir(parents=True)
        (prof / "history.json").write_text(json.dumps({"entries": entries}))
        m2 = Driver(binary, display, str(home), {"MORPH_LOG_LAYOUT": "1"})
        win = wait_for(lambda: m2.own_window(), 20)
        if not (win and wait_for(lambda: state(m2), 20)):
            m2.stop()
            return None, None
        time.sleep(1.0)
        win = m2.own_window() or win
        m2.activate(win)
        return m2, win

    def last(m2, pattern):
        found = m2.lines(pattern)
        return found[-1] if found else ""

    def omnibox(m2):
        mt = re.search(r'omnibox text=("(?:[^"\\]|\\.)*") selection=(\d+)\.\.(\d+)', last(m2, r"^morph-event omnibox "))
        return (json.loads(mt.group(1)), int(mt.group(2)), int(mt.group(3))) if mt else None

    def box(m2):
        mt = re.search(r"suggestions-box suggestions box left=(\d+) top=(\d+) width=(\d+) bar_bottom=(\d+) rows=(\d+)", last(m2, r"^morph-event suggestions-box"))
        return tuple(int(x) for x in mt.groups()) if mt else None

    m2, win = launch()
    if not m2:
        r.check(g, "a Morph opens with its page", False, "no window or page")
        return
    try:
        m2.key("ctrl+l")
        time.sleep(1.5)
        m2.type("git")
        time.sleep(0.8)
        shown = wait_for(lambda: box(m2), 6)
        r.check(g, "typing shows a list of suggestions", bool(shown), last(m2, r"^morph-event suggestions"))
        if shown:
            left, top, width, bar_bottom, rows = shown
            r.check(g, "the list sits below the address bar", top >= bar_bottom and rows >= 3, f"top {top}, bar bottom {bar_bottom}, rows {rows}")
            line = last(m2, r"^morph-event suggestions text=")
            r.check(g, "history ranks the most visited and recent page first, and the last row is the search", 'morph' in line and line.rstrip().endswith('"search"]'), line)
            if shots:
                Path(shots).mkdir(parents=True, exist_ok=True)
                screenshot(m2, Path(shots) / "suggestions-typing.png")
            # Pixels: the list is drawn under the bar, the page beside and below it.
            try:
                from PIL import Image

                png = Path(tempfile.mkdtemp(prefix="morph-xvfb-omnibox-")) / "list.png"
                screenshot(m2, png)
                img = Image.open(png).convert("RGB")
                gx, gy, _, _ = m2.geometry(win)
                f = img.width / (WaylandMorph.layout[0] if isinstance(m2, WaylandMorph) else img.width)
                at = lambda x, y: img.getpixel((int((gx + x) * f), int((gy + y) * f)))  # noqa: E731
                inside = luminance(at(left + width // 2, top + 6))
                bottom = next((y for y in range(top + 10, top + 600) if luminance(at(left + 6, y)) > 170), None)
                corner = luminance(at(left + 1, bottom - 1)) if bottom else 0
                r.check(g, "the list is drawn over the page with no black backing", inside < 120 and corner > 170, f"inside {inside:.0f}, corner {corner:.0f}")
            except ImportError:
                pass
        r.check(g, "the top match completes inline as selected text", omnibox(m2) == ("github.com", 3, 10), str(omnibox(m2)))
        m2.key("Right")
        time.sleep(0.4)
        r.check(g, "Right accepts the completion", omnibox(m2) == ("github.com", 10, 10), str(omnibox(m2)))
        # No completion: Right is plain cursor movement.
        m2.key("ctrl+a")
        m2.type("zzz")
        time.sleep(0.5)
        r.check(g, "text that matches nothing has no completion", omnibox(m2) == ("zzz", 3, 3), str(omnibox(m2)))
        m2.key("Home", "Right")
        time.sleep(0.4)
        r.check(g, "Right without a completion moves the cursor", omnibox(m2) == ("zzz", 1, 1), str(omnibox(m2)))
        # Backspace takes the completion away without completing again.
        m2.key("ctrl+a")
        m2.type("gitl")
        time.sleep(0.6)
        r.check(g, "a phrase from another site completes too", omnibox(m2) == ("gitlab.com", 4, 10), str(omnibox(m2)))
        m2.key("BackSpace")
        time.sleep(0.5)
        r.check(g, "Backspace deletes the completion and does not complete again", omnibox(m2) == ("gitl", 4, 4), str(omnibox(m2)))
        # Escape closes the list and keeps the typed text.
        m2.key("Escape")
        time.sleep(0.6)
        r.check(g, "Escape closes the list", bool(m2.lines(r"^morph-event suggestions closed")), "")
        r.check(g, "Escape keeps the typed text in the bar", (omnibox(m2) or ("",))[0] == "gitl", str(omnibox(m2)))
    finally:
        m2.stop()

    # Down and Enter open a row.
    m2, win = launch()
    if m2:
        try:
            m2.key("ctrl+l")
            time.sleep(1.5)
            m2.type("weird")
            time.sleep(0.8)
            m2.key("Down", "Down")
            m2.key("Return")
            ok = wait_for(lambda: m2.lines(r"^morph-event suggestion opened https://github.com/WeirdBrains/weirdnet"), 6)
            r.check(g, "Down moves the selection and Enter opens the row", bool(ok), str(m2.lines(r"^morph-event suggestion")))
        finally:
            m2.stop()

    # A click opens a row; the search row searches with the configured engine.
    m2, win = launch()
    if m2:
        try:
            m2.key("ctrl+l")
            time.sleep(1.5)
            m2.type("exam")
            time.sleep(0.8)
            shown = wait_for(lambda: box(m2), 6)
            if shown:
                left, top, width, _, rows = shown
                m2.click(win, left + 60, top + 4 + 17 + 34 * (rows - 1))
                ok = wait_for(lambda: m2.lines(r"^morph-event suggestion opened https://\S*[?&]q=exam$"), 6)
                r.check(g, "clicking the search row searches with the configured engine", bool(ok), str(m2.lines(r"^morph-event suggestion")))
            else:
                r.check(g, "clicking the search row searches with the configured engine", False, "no list")
        finally:
            m2.stop()
    m2, win = launch()
    if m2:
        try:
            m2.key("ctrl+l")
            time.sleep(1.5)
            m2.type("exam")
            time.sleep(0.8)
            shown = wait_for(lambda: box(m2), 6)
            if shown:
                left, top, _, _, _ = shown
                m2.click(win, left + 60, top + 4 + 17)
                ok = wait_for(lambda: m2.lines(r"^morph-event suggestion opened https://example.org/"), 6)
                r.check(g, "clicking a row opens it", bool(ok), str(m2.lines(r"^morph-event suggestion")))
            else:
                r.check(g, "clicking a row opens it", False, "no list")
        finally:
            m2.stop()


def relays_layout(r, binary, display, shots=None):
    """In Settings → Relays the Weirdnet discovery switch and the manual
    peer input stay inside their row's card at the default width and at a
    narrow one (X11: the window is resized), with the help text wrapping and
    the controls dropping below it when there is no room."""
    home = tempfile.mkdtemp(prefix="morph-xvfb-relays-layout-")
    m2 = Driver(binary, display, home, {"MORPH_LOG_LAYOUT": "1"})

    def rect(name):
        found = m2.lines(rf"^morph-event settings-layout {name} ")
        return tuple(int(x) for x in found[-1].split()[-4:]) if found else None

    def inside(item):
        card, controls = rect(f"{item} card"), rect(f"{item} controls")
        if not (card and controls):
            return False, f"no layout for {item}: card {card} controls {controls}"
        # Horizontal containment is what clipped before; the row's height is the card's own (flex).
        ok = controls[0] >= card[0] and controls[0] + controls[2] <= card[0] + card[2] and controls[1] >= card[1]
        return ok, f"{item}: controls {controls} in card {card}"

    try:
        win = wait_for(lambda: m2.own_window(), 20)
        if not (win and wait_for(lambda: state(m2), 20)):
            r.check("relays", "a Morph opens with its page", False, "no window or page")
            return
        time.sleep(1.0)
        win = m2.own_window() or win
        m2.activate(win)
        m2.key("ctrl+comma", delay=0.8)
        m2.key("slash", delay=0.4)
        m2.type("relays")
        time.sleep(1.5)
        widths = [("default", None)]
        if not isinstance(m2, WaylandMorph):
            widths.append(("narrow", 520))
        for name, width in widths:
            if width:
                m2.xdo("windowsize", win, str(width), "700")
                time.sleep(1.5)
                m2.xdo("mousemove", "--window", win, "200", "300")
            for item in ("Discovery", "ManualPeers"):
                wait_for(lambda: rect(f"{item} card"), 8)
                ok, detail = inside(item)
                r.check("relays", f"the {item} row's controls stay inside its card at the {name} width", ok, detail)
            if shots:
                Path(shots).mkdir(parents=True, exist_ok=True)
                screenshot(m2, Path(shots) / ("relays-layout.png" if name == "default" else f"relays-layout-{name}.png"))
    finally:
        m2.stop()


def windows_check(r, binary, display, shots=None):
    """Several windows: Ctrl+N opens a second one and Ctrl+W on its only
    tab closes just that window; Shift+click opens a link in a new window;
    the tab menu's Move to new window takes a tab to a window of its own;
    a relaunch brings both windows back."""
    home = tempfile.mkdtemp(prefix="morph-xvfb-windows-")
    restore = {"MORPH_NO_RESTORE": "0"}
    m2 = Driver(binary, display, home, restore, args=[LINKS.as_uri()])
    opened = lambda: len(m2.lines(r"^morph-event window opened")) - len(m2.lines(r"^morph-event window closed"))  # noqa: E731
    titles = lambda: [m2.title(w) for w in m2.windows()]  # noqa: E731

    def shot(name):
        if shots:
            Path(shots).mkdir(parents=True, exist_ok=True)
            screenshot(m2, Path(shots) / f"windows-{name}.png")

    def other(win):
        return wait_for(lambda: next((w for w in m2.windows() if w != win), None), 8)

    try:
        win = wait_for(lambda: m2.own_window(), 20)
        ok = win and wait_for(lambda: any(t.startswith('L{"links":1}') for t in titles()), 20)
        r.check("windows", "a Morph opens with the link page", ok, str(titles()))
        if not ok:
            return
        time.sleep(1.0)
        win = m2.own_window() or win
        m2.activate(win)
        m2.key("ctrl+n", delay=1.5)
        r.check("windows", "Ctrl+N opens a second window", wait_for(lambda: opened() == 2 and len(m2.windows()) == 2), str(m2.lines("morph-event window")))
        shot("ctrl-n")
        second = other(win)
        if second:
            m2.activate(second)
            m2.key("ctrl+w", delay=1.5)
            ok = wait_for(lambda: opened() == 1 and len(m2.windows()) == 1)
            r.check("windows", "Ctrl+W on its only tab closes that window alone", ok and m2.proc.poll() is None, f"exit {m2.proc.poll()}, {m2.lines('morph-event window')}")
        if m2.proc.poll() is not None:
            return
        m2.activate(win)
        m2.click_with(win, 300, 400, mod="shift")
        ok = wait_for(lambda: opened() == 2 and any(t.startswith("M{") for t in titles()), 10)
        r.check("windows", "Shift+click on a link opens it in a new window", ok, f"{titles()} {m2.lines('morph-event (window|link)')}")
        shot("shift-click")
        r.check("windows", "and the link's page stays as it was", any(t.startswith('L{"links":1}') for t in titles()), str(titles()))
        second = other(win)
        if second:
            m2.activate(second)
            m2.click(second, 300, 400)
            m2.key("ctrl+w", delay=1.5)
            wait_for(lambda: opened() == 1)
        m2.activate(win)
        before = len(m2.events())
        m2.click_with(win, 300, 400, mod="ctrl")
        tab2 = wait_for(lambda: any(t == 2 and n == "TitleChanged" for t, n, _ in m2.events()[before:]), 10)
        r.check("windows", "Ctrl+click on a link opens it in a new tab", tab2 and opened() == 1, str(m2.events()[before:][-3:]))
        # The second tab's menu: Move to new window is the fourth entry.
        m2.click_with(win, 300, 17, button=3)
        time.sleep(0.8)
        shot("tab-menu")
        m2.key("Down", "Down", "Down", "Down", "Return", delay=0.3)
        ok = wait_for(lambda: m2.lines(r"^morph-event tab moves to a new window") and opened() == 2, 10)
        r.check("windows", "Move to new window takes the tab to a window of its own", ok, str(m2.lines("morph-event (window|tab moves)")))
        moved = wait_for(lambda: sorted(t[:2] for t in titles()) == ["L{", "M{"], 10)
        r.check("windows", "the moved page shows there, the link page stays", moved, str(titles()))
        shot("moved")
        tabs_json = Path(home) / "profiles" / "default" / "tabs.json"
        saved = lambda: json.loads(tabs_json.read_text()) if tabs_json.exists() else {}  # noqa: E731
        ok = wait_for(lambda: len(saved().get("windows", [])) == 2)
        r.check("windows", "tabs.json keeps both windows", ok, str(saved()))
        m2.activate(win)
        m2.key("ctrl+q", delay=1.0)
        code = wait_for(lambda: m2.proc.poll() is not None and str(m2.proc.poll()), 8)
        r.check("windows", "Ctrl+Q quits", code == "0", f"exit {m2.proc.poll()}")
        m2.stop()
        m2 = Driver(binary, display, home, restore, args=[])
        ok = wait_for(lambda: opened() == 2 and sorted(t[:2] for t in titles()) == ["L{", "M{"], 20)
        r.check("windows", "a relaunch brings both windows back", ok, f"{titles()} {m2.lines('morph-event window')}")
        shot("restored")
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
    # The maximize button toggles by the window's real state.
    m.click(win, w - 56, 18)
    r.check("frame", "the maximize button maximizes", wait_for(lambda: (state(m) or {}).get("w", 0) > 1300), str(state(m)))
    _, _, w2, _ = m.geometry(win)
    m.click(win, w2 - 56, 18)
    r.check("frame", "the maximize button again restores", wait_for(lambda: (state(m) or {}).get("w") == 1190), str(state(m)))
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
    # A Morph just started may still be taking its size (Wayland, scale 2).
    _, s0 = settled_layout(m, win, state(m) or {})
    m.click(win, 300, 76 + 300)
    w0 = (state(m) or s0).get("w", 0)
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


def qr_texts(path):
    """The QR codes a screenshot holds, decoded (zxing-cpp); None when no
    decoder is installed (`pip install zxing-cpp pillow`)."""
    try:
        import zxingcpp
        from PIL import Image
    except ImportError:
        return None
    return [res.text for res in zxingcpp.read_barcodes(Image.open(path)) if res.format == zxingcpp.BarcodeFormat.QRCode]


def screenshot(m, path):
    """The whole screen as a PNG at `path` (grim on Wayland; ImageMagick's
    import, else Pillow, on X11). Nothing is written when neither works."""
    if isinstance(m, WaylandMorph):
        command = ["grim", str(path)] if shutil.which("grim") else ["wayshot", str(path)]
    else:
        command = ["import", "-window", "root", str(path)]
    try:
        subprocess.run(command, env=m.env, capture_output=True, timeout=20)
        return
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    if not isinstance(m, WaylandMorph):
        try:
            from PIL import ImageGrab

            ImageGrab.grab(xdisplay=m.env["DISPLAY"]).save(path)
        except Exception:
            pass


def start_screen(m, r, win, top):
    """A tab with nothing in it shows Morph's start screen (as the phone app
    does): the dark background, the white lockup and text in the middle, and
    the faint cyan and magenta glitch bars (55% over the background), not an
    empty white web view."""
    try:
        from PIL import Image
    except ImportError:
        if os.environ.get("MORPH_REQUIRE_QR") == "1":
            r.check("start", "a blank tab shows the start screen", False, "no Pillow (pip install pillow)")
        else:
            print("SKIP  start   a blank tab shows the start screen (no Pillow)", flush=True)
        return
    path = Path(tempfile.mkdtemp(prefix="morph-xvfb-start-")) / "screen.png"
    detail = "no screenshot"
    for _ in range(4):
        time.sleep(0.8)
        screenshot(m, path)
        if not path.exists():
            continue
        img = Image.open(path).convert("RGB")
        wx, wy, ww, wh = m.geometry(win)
        # Wayland: sway gives logical pixels, grim shoots the output in physical ones.
        f = 1600 / WaylandMorph.layout[0] if isinstance(m, WaylandMorph) else 1.0
        box = (int(wx * f), int((wy + top) * f), int((wx + ww) * f), int((wy + wh) * f))
        page = img.crop(box)
        px = page.load()
        pw, ph = page.size
        near = lambda c, want, tol=14: all(abs(a - b) <= tol for a, b in zip(c, want))
        corners = [px[x, y] for x, y in ((24, 24), (pw - 25, 24), (24, ph - 25), (pw - 25, ph - 25), (pw // 2, 24))]
        dark = sum(near(c, (13, 13, 18), 10) for c in corners)
        cyan = magenta = white = 0
        for y in range(0, ph, 2):
            for x in range(0, pw, 2):
                c = px[x, y]
                if near(c, (6, 132, 148)):
                    cyan += 1
                elif near(c, (146, 30, 126)):
                    magenta += 1
                elif min(c) > 225:
                    white += 1
        n = f * f
        detail = f"dark corners {dark}/5, bar pixels cyan {cyan} magenta {magenta}, white {white} (scale {f:g})"
        if dark >= 4 and cyan >= 120 * n and magenta >= 120 * n and white >= 400 * n:
            break
    else:
        r.check("start", "a blank tab shows the start screen", False, detail)
        return
    r.check("start", "a blank tab shows the start screen", True, detail)


def check_qr(r, area, what, m, expected):
    """The screen shows a QR code that decodes to `expected` (it painted,
    and it is the right one)."""
    path = Path(tempfile.mkdtemp(prefix="morph-xvfb-qr-")) / "screen.png"
    texts = None
    for _ in range(3):
        screenshot(m, path)
        texts = qr_texts(path) if path.exists() else []
        if texts is None or expected in texts:
            break
        time.sleep(1.0)
    if texts is None:
        if os.environ.get("MORPH_REQUIRE_QR") == "1":
            r.check(area, what, False, "no QR decoder (pip install zxing-cpp pillow)")
        else:
            print(f"SKIP  {area:7} {what} (no QR decoder)", flush=True)
        return
    r.check(area, what, expected in texts, f"decoded {[t[:40] for t in texts]}")


def identity_link_invite(m, r, binary, display, user_id):
    """The other direction: the device with the identity shows a QR under
    Add a device (a), a fresh Morph pastes its link on its own link screen
    (4), and after the password and the same code check the new one gets
    the identity."""
    win, w0 = open_identity(m, r, "invite")
    # Only an invite this Add a device logs (an earlier one is stale).
    before = len(m.lines(r"morph-event link invite="))
    m.key("a", delay=1.5)
    invite = wait_for(lambda: [line.split("invite=", 1)[1].strip() for line in m.lines(r"morph-event link invite=")[before:]], 20)
    r.check("invite", "Add a device shows a code of its own", invite, "no invite logged")
    if not invite:
        return
    check_qr(r, "invite", "its QR code paints and reads back as its link", m, invite[-1])
    m4 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-invite-"), LINK_ENV)
    try:
        r.check("invite", "a new device (another home) opens", wait_for(lambda: state(m4), 20))
        new_win, _ = open_identity(m4, r, "invite")
        m4.key("4", delay=1.5)
        offer = wait_for(lambda: m4.lines(r"morph-event link offer="))
        if offer:
            check_qr(r, "invite", "the new device's QR code paints and reads back as its link", m4, offer[-1].split("offer=", 1)[1].strip())
        fill(m4, invite[-1])
        r.check("invite", "pasting the code there reaches this device", wait_for(lambda: m.lines(r"morph-event link answered"), 15), "no answer")
        # This device still shows Add a device, its password field focused:
        # the password, Enter, and Enter on the empty link field pairs with
        # the link that came back.
        m.activate(win)
        fill(m, PASSWORD)
        m.key("Return", delay=1.0)
        time.sleep(3.0)
        m.key("Return", delay=1.0)
        m4.activate(new_win)
        m4.key("Return", delay=3.0)
        fill(m4, PASSWORD)
        got = wait_for(lambda: identity_file(m4, "default").get("user_id"), 20)
        r.check("invite", "after both confirm, the new device has the same identity", got and got == user_id, f"{got} vs {user_id}")
    finally:
        m4.stop()
    m.activate(win)
    for _ in range(3):
        if (state(m) or {}).get("w") == w0:
            break
        m.key("Escape", delay=0.8)


def identity_link(m, r, binary, display, user_id):
    """Device linking: a fresh Morph (another home) offers a link with 4; the
    first adds it (a, password, link); both show the same code, Enter on
    each confirms, and the new one chooses its password and gets the
    identity."""
    m3 = Driver(binary, display, tempfile.mkdtemp(prefix="morph-xvfb-link-"), LINK_ENV)
    win, w0 = None, 0
    try:
        r.check("link", "a new device (another home) opens", wait_for(lambda: state(m3), 20))
        new_win, _ = open_identity(m3, r, "link")
        m3.key("4", delay=1.5)
        offer = wait_for(lambda: [line.split("offer=", 1)[1].strip() for line in m3.lines(r"morph-event link offer=")], 15)
        r.check("link", "4 shows a link (and its QR code)", offer, "no offer logged")
        want = "reserved" if LOCAL_RELAY else "(not-connected|connected|reserved)"
        relay = wait_for(lambda: m3.lines(rf"morph-event link relays state={want}"), 15)
        r.check("link", "the link screen shows the relay status line, from the running node" + (" (a slot on the local relay)" if LOCAL_RELAY else ""), relay, str(m3.lines(r"morph-event link relays")))
        if relay:
            print(f"INFO  link    relay line: {m3.lines(r'morph-event link relays')[-1]}", flush=True)
        if not offer:
            return
        check_qr(r, "link", "the offer's QR code paints and reads back as its link", m3, offer[0])
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
    r.check("error", "Alt+Left leaves the error page for the page under it", back, f"{m.title(win)} {state(m, tab=2)} {focus_info(m)}")


def searched_since(m, since):
    return [a for t, name, a in m.events()[since:] if name == "UrlChanged" and ("google.com/search" in (a or "") or "duckduckgo.com/?q" in (a or ""))]


def own_addresses(m, r, win, top):
    """morph:// and weirdnet-<app>:// addresses never search: an unknown
    morph:// page shows Morph's page listing the real ones, a Weirdnet app
    link whose name finds no one says so (a call over Weirdnet: "alice" has
    no _weirdnet TXT record), and an invalid app name says that; Alt+Left
    goes back from each."""

    for typed, title, label, wait in [
        ("morph://nope", r"morph://nope is not a Morph page", "an unknown morph:// page shows the list of Morph's pages", 6),
        (
            "weirdnet-muninn://alice/notes?token=x",
            r"Couldn't (find|reach) alice",
            "a weirdnet-<app>:// link is called over Weirdnet and says it couldn't find alice",
            60,
        ),
        ("weirdnet-no.good://alice/x", r"Not a valid Weirdnet app name", "an invalid app name says so", 6),
    ]:
        m.click(win, 300, top + 300)
        since = len(m.events())
        m.key("ctrl+l", delay=0.6)
        m.type(typed)
        m.key("Return", delay=1.0)
        shown = wait_for(lambda: re.match(title, m.title(win)), wait)
        r.check("address", label, shown, m.title(win))
        time.sleep(0.5)
        r.check("address", f"{typed.split('?')[0]} is not searched", not searched_since(m, since), str(searched_since(m, since)))
        m.key("alt+Left", delay=1.0)
        r.check("address", f"Alt+Left leaves the {typed.split(':')[0]} page", wait_for(lambda: not re.match(title, m.title(win))), m.title(win))
    called = m.lines(r"morph-event app-call weirdnet-muninn://alice/notes failed (not_found|unreachable|timed_out)")
    r.check("address", "the app link went to the app caller (no query in the log)", called and not m.lines(r"morph-event app-call .*token"), str(m.lines(r"morph-event app-call")[-3:]))


def go_to_keys(m, win, top, suffix=""):
    """Load keys.html in the current tab and wait until it reports a fresh
    state. After navigating away (app-call pages, Morph error pages) later
    checks still need this page's innerWidth and its window.open button."""
    since = len(m.events())
    m.click(win, 300, top + 300)
    m.key("ctrl+l", delay=0.6)
    m.type(PAGE.as_uri() + suffix)
    m.key("Return", delay=1.5)
    return wait_for(lambda: any(n == "TitleChanged" and "M{" in (a or "") for _, n, a in m.events()[since:]))


def app_calls(m, r, win, top):
    """weirdnet-morph-test://<peer>/… against the serve-app example behind
    the local relay: the app's HTML, JSON and image answers load in the tab
    under the app address, and a 404, a refusal and an app the peer doesn't
    serve each get Morph's page saying so. None is searched."""
    base = f"weirdnet-morph-test://{APP_PEER}"
    for path, title, logged, label in [
        ("/hello?x=1", r"Hello from morph-test", r"ok status=200 kind=html", "an app's HTML page loads through the relay, titled by the app"),
        ("/data.json", None, r"ok status=200 kind=json", "an app's JSON answer is shown"),
        ("/pic.png", None, r"ok status=200 kind=image", "an app's image is shown"),
        ("/missing", r"The app answered 404 Not Found", r"failed status", "a 404 with nothing to show says so"),
        ("/refuse", r"The app refused the call", r"failed refused", "a refusal says so"),
    ]:
        m.click(win, 300, top + 300)
        since = len(m.events())
        before = len(m.lines(r"morph-event app-call "))
        m.key("ctrl+l", delay=0.6)
        m.type(base + path)
        m.key("Return", delay=1.0)
        url = re.escape(base + path.split("?")[0])
        done = wait_for(lambda: len(m.lines(r"morph-event app-call ")) > before and m.lines(rf"morph-event app-call {url} {logged}"), 45)
        if title:
            done = done and wait_for(lambda: re.match(title, m.title(win)), 10)
        r.check("app", label, done, f"{m.title(win)} {m.lines(r'morph-event app-call')[-2:]}")
        r.check("app", f"{path.split('?')[0]} is not searched", not searched_since(m, since), str(searched_since(m, since)))
        if path.startswith("/hello"):
            shown = [a for t, name, a in m.events()[since:] if name == "UrlChanged"]
            r.check("app", "the engine got the app's page as a data: address (Morph never hands it the weirdnet- address)", any((a or "").strip("\"").startswith("data:text/html") for a in shown), str([(a or "")[:40] for a in shown]))
            r.check("app", "the query never reaches the log", not m.lines(r"morph-event app-call .*x=1"), "")
        m.key("alt+Left", delay=1.0)
    m.click(win, 300, top + 300)
    m.key("ctrl+l", delay=0.6)
    m.type(f"weirdnet-other-app://{APP_PEER}/")
    m.key("Return", delay=1.0)
    r.check("app", "an app the peer doesn't serve says so", wait_for(lambda: m.title(win).startswith(f"{APP_PEER} doesn't serve weirdnet-other-app://"), 45), m.title(win))
    m.key("alt+Left", delay=1.0)
    # The calls leave the tab on an app page. F11, window.open and the
    # profiles-panel width check all read keys.html's innerWidth (and the
    # popup click hits a button on that page), so load it again; otherwise
    # state() keeps the last keys.html title (stale width) and those checks
    # fail even though the shell itself is fine.
    r.check("app", "the keys page is showing again after the app calls", go_to_keys(m, win, top, "?after-app"), m.title(win))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", default=str(Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "release" / "morph"))
    ap.add_argument("--report")
    ap.add_argument("--no-wm", action="store_true")
    ap.add_argument("--wayland", action="store_true", help="run on a headless sway (native Wayland) instead of Xvfb")
    ap.add_argument("--scale", type=float, default=1.0, help="the Wayland output's scale")
    ap.add_argument("--input-bin", help="morph-wl-input (default: next to --bin)")
    ap.add_argument("--local-relay", help="a local relay to link through (weirdnet-device-link's local-relay example)")
    ap.add_argument("--app-server", help="a Weirdnet app behind the local relay (morph-services' serve-app example)")
    ap.add_argument("--only", choices=["downloads", "windows", "popups", "omnibox", "relays"], help="run just this check")
    ap.add_argument("--screenshots", help="save screenshots of checks that take them (X11) here")
    args = ap.parse_args()
    relay = start_local_relay(args.local_relay) if args.local_relay else None
    app = start_app_server(args.app_server) if relay and args.app_server else None
    try:
        return run(args)
    finally:
        for proc in (app, relay):
            if proc:
                proc.stdin.close()
                proc.terminate()


def start_app_server(binary):
    """Start the test app behind the local relay (it listens on nothing of
    its own, so Morph reaches it only through a circuit) on a temporary
    MORPH_HOME, and wait until the relay holds its slot."""
    global APP_PEER
    env = dict(os.environ, MORPH_HOME=tempfile.mkdtemp(prefix="morph-xvfb-app-"), MORPH_LINK_RELAYS=LINK_ENV["MORPH_LINK_RELAYS"])
    proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, env=env)
    peer = None
    for line in proc.stdout:
        line = line.strip()
        if line.startswith("peer "):
            peer = line.split(" ", 1)[1]
        if line == "ready":
            break
    if not peer:
        raise SystemExit("the app server did not start")
    APP_PEER = peer
    print(f"INFO  app  serving weirdnet-morph-test:// as {peer}", flush=True)
    return proc


def start_local_relay(binary):
    """Start the local relay and point every Morph at it: a dead address
    (port 1 refuses), then its TCP and QUIC addresses."""
    global LOCAL_RELAY
    proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    addrs = []
    for line in proc.stdout:
        line = line.strip()
        if line == "ready":
            break
        if line.startswith("relay "):
            addrs.append(line.split(" ", 1)[1])
    tcp = [a for a in addrs if "/tcp/" in a]
    quic = [a for a in addrs if "/quic-v1" in a]
    if not tcp or not quic:
        raise SystemExit(f"the local relay did not listen on TCP and QUIC: {addrs}")
    peer = tcp[0].rsplit("/p2p/", 1)[1]
    LINK_ENV.update(MORPH_LINK_RELAYS=",".join([f"/ip4/127.0.0.1/tcp/1/p2p/{peer}", tcp[0], quic[0]]), MORPH_RELAY_INFO="0")
    LOCAL_RELAY = True
    print(f"INFO  relays  local relay: {LINK_ENV['MORPH_LINK_RELAYS']}", flush=True)
    return proc


def run(args):

    global Driver
    xvfb, wm = None, None
    if args.wayland:
        Driver = WaylandMorph
        xvfb, display = start_sway(args.input_bin or str(Path(args.bin).with_name("morph-wl-input")), args.scale)
        wm = xvfb  # sway manages windows (full screen) itself
    else:
        display = ":" + str(70 + os.getpid() % 200)
        if display == ":100":  # someone's own virtual display
            display = ":270"
        xvfb = subprocess.Popen(["Xvfb", display, "-screen", "0", "1600x1000x24", "-ac", "-noreset"], stderr=subprocess.DEVNULL)
        time.sleep(1.5)
        if not args.no_wm and shutil.which("openbox"):
            wm = subprocess.Popen(["openbox"], env=dict(os.environ, DISPLAY=display), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(1.0)
    if args.only == "omnibox":
        r = Results()
        try:
            suggestions_check(r, args.bin, display, args.screenshots)
            return finish(r, args)
        finally:
            for proc in (wm, xvfb):
                if proc and proc.poll() is None:
                    proc.terminate()
    if args.only == "relays":
        r = Results()
        try:
            relays_layout(r, args.bin, display, args.screenshots)
            return finish(r, args)
        finally:
            for proc in (wm, xvfb):
                if proc and proc.poll() is None:
                    proc.terminate()
    if args.only == "popups":
        r = Results()
        try:
            popup_backing(r, args.bin, display, args.screenshots)
            return finish(r, args)
        finally:
            for proc in (wm, xvfb):
                if proc and proc.poll() is None:
                    proc.terminate()
    if args.only == "windows":
        r = Results()
        try:
            windows_check(r, args.bin, display, args.screenshots)
            return finish(r, args)
        finally:
            for proc in (wm, xvfb):
                if proc and proc.poll() is None:
                    proc.terminate()
    if args.only == "downloads":
        r = Results()
        try:
            base = serve_tests()
            downloads_panel(r, args.bin, display, base, args.screenshots)
            downloads_js_login(r, args.bin, display, base, True)
            downloads_js_login(r, args.bin, display, base, False)
            return finish(r, args)
        finally:
            for proc in (wm, xvfb):
                if proc and proc.poll() is None:
                    proc.terminate()
    home = tempfile.mkdtemp(prefix="morph-xvfb-")
    m = Driver(args.bin, display, home, LINK_ENV)
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
        start_screen(m, r, win, top)
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
        own_addresses(m, r, win, top)
        if APP_PEER:
            app_calls(m, r, win, top)
        if wm:
            w0 = (state(m, tab=2) or {}).get("w", 0)
            m.click(win, 300, top + 300)
            m.key("F11", delay=0.8)
            w1 = wait_for(lambda: (state(m, tab=2) or {}).get("w", 0) > w0 and (state(m, tab=2) or {}).get("w"))
            r.check("shell", "F11 toggles full screen", w1 and w1 > w0, f"{w0} -> {(state(m, tab=2) or {}).get('w')}")
            m.key("F11", delay=0.8)
            w2 = wait_for(lambda: (state(m, tab=2) or {}).get("w", 0) < (w1 or 0) and (state(m, tab=2) or {}).get("w"))
            r.check("shell", "F11 again leaves full screen", w2 and w2 < w1, f"{w1} -> {(state(m, tab=2) or {}).get('w')}")

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
        m = Driver(args.bin, display, home, LINK_ENV)
        settings_relaunch(m, r, w0, h0)
        single_instance(m, r, args.bin)
        bridge_round_trip(m, r, args.bin)
        popup_close(m, r, args.bin)
        media(m, r, args.bin)
        gdk_scale_ignored(m, r, args.bin, display)
        render_check(r, args.bin, display)
        base = serve_tests()
        favicon(m, r, args.bin, base)
        busy_page(r, args.bin, display, base)
        long_address(r, args.bin, display, base)
        hover_page(r, args.bin, display, base)
        file_picker(r, args.bin, display, base)
        download(r, args.bin, display, base)
        downloads_panel(r, args.bin, display, base, args.screenshots)
        downloads_js_login(r, args.bin, display, base, True)
        downloads_js_login(r, args.bin, display, base, False)
        select_menu(r, args.bin, display, base)
        page_clipboard(r, args.bin, display, base, args.wayland)
        main_menu(r, args.bin, display)
        popup_backing(r, args.bin, display, args.screenshots)
        suggestions_check(r, args.bin, display, args.screenshots)
        relays_layout(r, args.bin, display, args.screenshots)
        windows_check(r, args.bin, display, args.screenshots)
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
            identity_link_invite(m, r, args.bin, display, user_id)
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
