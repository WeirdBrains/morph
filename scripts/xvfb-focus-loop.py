#!/usr/bin/env python3
"""Open and close a panel (or the find bar) N times in Xvfb and check that the
first key typed after each close reaches the page.

    scripts/xvfb-focus-loop.py --bin <morph> [--runs 50] [--key ctrl+comma]

Exits non-zero if any run loses the key. Uses scripts/xvfb-check.py's helpers.
"""
import argparse
import importlib.util
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

spec = importlib.util.spec_from_file_location("xc", Path(__file__).with_name("xvfb-check.py"))
xc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(xc)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--bin", required=True)
    ap.add_argument("--runs", type=int, default=50)
    ap.add_argument("--key", default="ctrl+comma", help="opens the panel; Escape closes it")
    args = ap.parse_args()
    display = ":" + str(70 + (os.getpid() + 101) % 200)
    xvfb = subprocess.Popen(["Xvfb", display, "-screen", "0", "1600x1000x24", "-ac", "-noreset"], stderr=subprocess.DEVNULL)
    time.sleep(1.5)
    wm = subprocess.Popen(["openbox"], env=dict(os.environ, DISPLAY=display), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1)
    m = xc.Morph(args.bin, display, tempfile.mkdtemp(prefix="morph-focus-loop-"))
    lost = 0
    try:
        win = xc.wait_for(lambda: m.window("Morph"), 20)
        xc.wait_for(lambda: xc.state(m), 20)
        m.click(win, 300, 376)
        letters = "abcdefghijklmnopqrstuvwxyz"
        for i in range(args.runs):
            k = letters[i % 26]
            m.key(args.key, delay=0.8)
            m.key("Escape", delay=0.3)
            m.key(k)
            ok = xc.wait_for(lambda: (xc.state(m) or {}).get("c") == "Key" + k.upper(), 3)
            lost += not ok
            print(f"{i + 1:3} {'ok' if ok else 'LOST'} {k}", flush=True)
    finally:
        m.stop()
        wm.terminate()
        xvfb.terminate()
    print(f"\n{args.runs - lost} of {args.runs} keys reached the page")
    return 1 if lost else 0


if __name__ == "__main__":
    sys.exit(main())
