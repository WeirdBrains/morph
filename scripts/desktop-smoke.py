#!/usr/bin/env python3
"""Smoke test for a release build of the desktop shell on macOS and Windows
(and Linux with a display): `morph --version`, then a real window whose first
tab loads a page from a local HTTP server. The page asks Rust for a pong
through the page bridge, changes its own address and sets its title; the
shell's page events (MORPH_LOG_EVENTS=1) must show all three.

    python3 scripts/desktop-smoke.py path/to/morph [--version 0.2.0-alpha.8] [--screenshot out.png]

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
    proc = subprocess.Popen([a.binary, url], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env, text=True, encoding="utf-8", errors="replace")
    lines = queue.Queue()
    threading.Thread(target=lambda: [lines.put(line) for line in proc.stdout], daemon=True).start()

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
    proc.kill()
    proc.wait(timeout=30)
    server.shutdown()
    print("--- morph output ---")
    print("\n".join(log[-80:]))
    print(f"{len(failures)} check(s) failed" if failures else "all checks passed")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
