#!/usr/bin/env python3
"""Clicks the left mouse button at X,Y on one X display, through the XTEST extension: how
run-client.sh plays a player's click on the client's own UI (M1 Task 10). Stdlib only (ctypes with
libX11 and libXtst). It sends input to that display and nothing else; it never touches the client.

    xclick.py --display :99 X Y
"""
import argparse
import ctypes
import ctypes.util
import sys
import time


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--display", required=True)
    ap.add_argument("x", type=int)
    ap.add_argument("y", type=int)
    a = ap.parse_args()
    x11 = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
    xtst = ctypes.CDLL(ctypes.util.find_library("Xtst") or "libXtst.so.6")
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    for f in (x11.XFlush, x11.XCloseDisplay):
        f.argtypes = [ctypes.c_void_p]
    xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    dpy = x11.XOpenDisplay(a.display.encode())
    if not dpy:
        print(f"xclick: can't open display {a.display}", file=sys.stderr)
        return 1
    try:
        xtst.XTestFakeMotionEvent(dpy, -1, a.x, a.y, 0)
        x11.XFlush(dpy)
        time.sleep(0.1)
        xtst.XTestFakeButtonEvent(dpy, 1, 1, 0)
        x11.XFlush(dpy)
        time.sleep(0.05)
        xtst.XTestFakeButtonEvent(dpy, 1, 0, 0)
        x11.XFlush(dpy)
    finally:
        x11.XCloseDisplay(dpy)
    return 0


if __name__ == "__main__":
    sys.exit(main())
