#!/usr/bin/env python3
"""Presses keys on one X display through the XTEST extension: how run-client.sh plays a player's
key presses and typing on the client's own UI (M3 Task 8). Stdlib only (ctypes with libX11 and
libXtst). It sends input to that display and nothing else; it never touches the client.

    xkey.py --display :99 --key Return       one X keysym (Return, space, Tab, BackSpace, a letter)
    xkey.py --display :99 --type "Lin Two"   printable ASCII, one key at a time, Shift as needed
"""
import argparse
import ctypes
import ctypes.util
import sys
import time


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--display", required=True)
    what = ap.add_mutually_exclusive_group(required=True)
    what.add_argument("--key")
    what.add_argument("--type")
    a = ap.parse_args()
    if a.type is not None and not (a.type and all(" " <= c <= "~" for c in a.type)):
        print("xkey: --type takes printable ASCII only", file=sys.stderr)
        return 2
    x11 = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
    xtst = ctypes.CDLL(ctypes.util.find_library("Xtst") or "libXtst.so.6")
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    for f in (x11.XFlush, x11.XCloseDisplay):
        f.argtypes = [ctypes.c_void_p]
    x11.XStringToKeysym.restype = ctypes.c_ulong
    x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
    x11.XKeysymToKeycode.restype = ctypes.c_ubyte
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XkbKeycodeToKeysym.restype = ctypes.c_ulong
    x11.XkbKeycodeToKeysym.argtypes = [ctypes.c_void_p, ctypes.c_ubyte, ctypes.c_int, ctypes.c_int]
    xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    dpy = x11.XOpenDisplay(a.display.encode())
    if not dpy:
        print(f"xkey: can't open display {a.display}", file=sys.stderr)
        return 1
    try:
        if a.key is not None:
            sym = x11.XStringToKeysym(a.key.encode())
            if not sym:
                print(f"xkey: no keysym named {a.key!r}", file=sys.stderr)
                return 2
            syms = [sym]
        else:
            # Printable ASCII keysyms equal their character codes (X11 keysymdef.h, Latin 1).
            syms = [ord(c) for c in a.type]
        shift = x11.XKeysymToKeycode(dpy, x11.XStringToKeysym(b"Shift_L"))
        for sym in syms:
            code = x11.XKeysymToKeycode(dpy, sym)
            if not code:
                print(f"xkey: keysym {sym:#x} has no key on this display", file=sys.stderr)
                return 1
            # Shift when the keysym sits at the key's second level (an upper-case letter, "_", ...).
            shifted = x11.XkbKeycodeToKeysym(dpy, code, 0, 0) != sym
            if shifted:
                xtst.XTestFakeKeyEvent(dpy, shift, 1, 0)
            xtst.XTestFakeKeyEvent(dpy, code, 1, 0)
            x11.XFlush(dpy)
            time.sleep(0.03)
            xtst.XTestFakeKeyEvent(dpy, code, 0, 0)
            if shifted:
                xtst.XTestFakeKeyEvent(dpy, shift, 0, 0)
            x11.XFlush(dpy)
            time.sleep(0.03)
    finally:
        x11.XCloseDisplay(dpy)
    return 0


if __name__ == "__main__":
    sys.exit(main())
