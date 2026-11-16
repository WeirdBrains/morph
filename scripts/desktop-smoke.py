#!/usr/bin/env python3
"""Smoke test for a release build of the desktop shell on macOS and Windows
(and Linux with a display): `morph --version`, then a real window whose first
tab loads a page from a local HTTP server. The page asks Rust for a pong
through the page bridge, changes its own address and sets its title; the
shell's page events (MORPH_LOG_EVENTS=1) must show all three.

    python3 scripts/desktop-smoke.py path/to/morph [--version 0.2.0-alpha.8] [--screenshot out.png]

Then the Identity panel: the shortcut pressed as a user would (Cmd+Shift+L
through System Events on macOS, Ctrl+Shift+L through SendKeys on Windows)
must open it, and so must a second launch that asks for it through
MORPH_TEST_ACTIONS (the same path a menu or button takes).

Exits 0 when every check passed. Uses a throwaway MORPH_HOME.
"""
import argparse
import http.server
import os
import queue
import re
import subprocess
import sys
import tempfile
import threading
import time

PAGE = b"""<!doctype html>
<html><head><meta charset="utf-8"><title>morph-smoke-loading</title></head>
<body style="font: 32px sans-serif; background: #1e6fd9; color: white">
<h1>Morph smoke test</h1><p id="state">loading</p>
<script>
addEventListener("load", async () => {
  let reply = "no-bridge";
  try { reply = (await window.MorphBridge.ping()).type; } catch (e) { reply = "error-" + e.message; }
  history.pushState({}, "", "/pushed");
  document.getElementById("state").textContent = reply;
  document.title = "morph-smoke " + reply;
});
</script></body></html>"""


class Page(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(PAGE)))
        self.end_headers()
        self.wfile.write(PAGE)

    def log_message(self, *args):
        pass


def screenshot(path):
    try:
        if sys.platform == "darwin":
            subprocess.run(["screencapture", "-x", path], timeout=30, check=False)
        elif sys.platform == "win32":
            ps = (
                "Add-Type -AssemblyName System.Windows.Forms,System.Drawing;"
                "$b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds;"
                "$i=New-Object System.Drawing.Bitmap $b.Width,$b.Height;"
                "$g=[System.Drawing.Graphics]::FromImage($i);"
                "$g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size);"
                f"$i.Save('{path}')"
            )
            subprocess.run(["powershell", "-NoProfile", "-Command", ps], timeout=60, check=False)
        print(f"screenshot: {path} ({'saved' if os.path.exists(path) else 'not saved'})")
    except Exception as e:  # a screenshot is a bonus, never a failure
        print(f"screenshot failed: {e}")


def qr_texts(path):
    """QR codes in a screenshot (zxing-cpp); None without a decoder."""
    try:
        import zxingcpp
        from PIL import Image
    except ImportError:
        return None
    if not os.path.exists(path):
        return []
    return [r.text for r in zxingcpp.read_barcodes(Image.open(path)) if r.format == zxingcpp.BarcodeFormat.QRCode]


def press_identity_shortcut():
    """Press the Identity shortcut in the frontmost app, as a user would."""
    if sys.platform == "darwin":
        script = 'tell application "System Events" to keystroke "l" using {command down, shift down}'
        return subprocess.run(["osascript", "-e", script], capture_output=True, text=True, timeout=30)
    if sys.platform == "win32":
        ps = "$w = New-Object -ComObject WScript.Shell; $null = $w.AppActivate('Morph'); Start-Sleep -Milliseconds 500; $w.SendKeys('^+l')"
        return subprocess.run(["powershell", "-NoProfile", "-Command", ps], capture_output=True, text=True, timeout=60)
    return None


def activate(proc):
    """Bring Morph to the front (macOS: by its process id)."""
    if sys.platform == "darwin":
        script = f'tell application "System Events" to set frontmost of (first process whose unix id is {proc.pid}) to true'
        subprocess.run(["osascript", "-e", script], capture_output=True, text=True, timeout=30)


def wait_line(lines, log, rx, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            line = lines.get(timeout=0.5)
        except queue.Empty:
            continue
        log.append(line.rstrip())
        if rx.search(line):
            return True
    return False


def launch(binary, url, env):
    proc = subprocess.Popen([binary, url], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env, text=True, encoding="utf-8", errors="replace")
    lines = queue.Queue()
    threading.Thread(target=lambda: [lines.put(line) for line in proc.stdout], daemon=True).start()
    return proc, lines


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("binary")
    ap.add_argument("--version", help="the version `morph --version` must print")
    ap.add_argument("--screenshot")
    ap.add_argument("--timeout", type=float, default=120)
    a = ap.parse_args()
    failures = []

    def check(name, ok, detail=""):
        print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  ({detail})" if detail and not ok else ""))
        if not ok:
            failures.append(name)

    out = subprocess.run([a.binary, "--version"], capture_output=True, text=True, timeout=60)
    printed = out.stdout.strip()
    check("morph --version", out.returncode == 0 and printed.startswith("morph ") and (not a.version or printed == f"morph {a.version}"), repr(printed) + out.stderr)

    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Page)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_address[1]}/smoke"
    home = tempfile.mkdtemp(prefix="morph-smoke-")
    env = dict(os.environ, MORPH_HOME=home, MORPH_LOG_EVENTS="1")
    proc, lines = launch(a.binary, url, env)

    want = {
        "the page loads": re.compile(r"LoadFinished"),
        "the page's title reaches the shell": re.compile(r'TitleChanged\("morph-smoke'),
        "the page bridge answers ping with pong": re.compile(r'TitleChanged\("morph-smoke pong"\)'),
        "an address the page set itself (pushState) reaches the shell": re.compile(r'UrlChanged\("http://127\.0\.0\.1:\d+/pushed"\)'),
    }
    seen, log = set(), []
    deadline = time.monotonic() + a.timeout
    while len(seen) < len(want) and time.monotonic() < deadline and proc.poll() is None:
        try:
            line = lines.get(timeout=0.5)
        except queue.Empty:
            continue
        log.append(line.rstrip())
        seen.update(name for name, rx in want.items() if rx.search(line))
    alive = proc.poll() is None
    check("the window stays up", alive, f"exit code {proc.returncode}")
    for name in want:
        check(name, name in seen)
    if a.screenshot and alive:
        time.sleep(2)
        screenshot(a.screenshot)
    panel = re.compile(r"morph-event panel morph\.identity")
    if alive and sys.platform in ("darwin", "win32"):
        activate(proc)
        time.sleep(1)
        pressed = press_identity_shortcut()
        opened = wait_line(lines, log, panel, 15)
        detail = (pressed.stderr or "").strip() if pressed else ""
        # A runner without accessibility rights cannot send keys; that is
        # the runner, not Morph, so it only warns.
        if not opened and pressed is not None and pressed.returncode != 0:
            print(f"WARN  could not press the shortcut ({detail}); skipped")
        else:
            check("the Identity shortcut opens the Identity panel", opened, detail)
        if opened and a.screenshot:
            time.sleep(1.5)
            screenshot(a.screenshot.replace(".png", "-identity.png"))
    proc.kill()
    proc.wait(timeout=30)
    # The panel itself opens (no keystroke: the action, as a button sends it).
    env2 = dict(env, MORPH_TEST_ACTIONS="shell.show_identity", MORPH_HOME=tempfile.mkdtemp(prefix="morph-smoke-id-"))
    proc2, lines2 = launch(a.binary, url, env2)
    check("the Identity panel opens", wait_line(lines2, log, panel, a.timeout), "no panel event")
    time.sleep(2)
    check("the window stays up with the Identity panel open", proc2.poll() is None, f"exit code {proc2.returncode}")
    if a.screenshot and proc2.poll() is None:
        screenshot(a.screenshot.replace(".png", "-identity-panel.png"))
    proc2.kill()
    proc2.wait(timeout=30)
    # The device-link QR paints: the offer screen opens by itself
    # (MORPH_TEST_IDENTITY) and the screenshot must decode to its link.
    env3 = dict(env2, MORPH_TEST_IDENTITY="offer", MORPH_HOME=tempfile.mkdtemp(prefix="morph-smoke-qr-"))
    proc3, lines3 = launch(a.binary, url, env3)
    offer = re.compile(r"morph-event link offer=(\S+)")
    got = None
    deadline = time.monotonic() + a.timeout
    while got is None and time.monotonic() < deadline and proc3.poll() is None:
        try:
            line = lines3.get(timeout=0.5)
        except queue.Empty:
            continue
        log.append(line.rstrip())
        m = offer.search(line)
        got = m.group(1) if m else None
    check("Link from another device shows an offer", got, "no offer logged")
    if got:
        activate(proc3)
        texts = None
        shot = (a.screenshot or os.path.join(tempfile.mkdtemp(), "smoke.png")).replace(".png", "-link-qr.png")
        for _ in range(4):
            time.sleep(2)
            screenshot(shot)
            texts = qr_texts(shot)
            if texts is None or got in texts:
                break
        if texts is None and os.environ.get("MORPH_REQUIRE_QR") != "1":
            print("SKIP  the link QR code paints (no QR decoder: pip install zxing-cpp pillow)")
        else:
            check("the link QR code paints and reads back as its link", texts is not None and got in texts, f"decoded {[t[:40] for t in texts or []]}")
    proc3.kill()
    proc3.wait(timeout=30)
    server.shutdown()
    print("--- morph output ---")
    print("\n".join(log[-80:]))
    print(f"{len(failures)} check(s) failed" if failures else "all checks passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
