#!/usr/bin/env python3
"""Presses keys on one X display through the XTEST extension: how run-client.sh plays a player's
key presses and typing on the client's own UI (M3 Task 8). Stdlib only (ctypes with libX11 and
libXtst). It sends input to that display and nothing else; it never touches the client.

    xkey.py --display :99 --key Return       one X keysym (Return, space, Tab, BackSpace, a letter)
    xkey.py --display :99 --key w --hold 1500
                                             the same, held down 1,500 ms (1 to 60,000), as a player
                                             holds a movement key; SIGTERM releases it first
    xkey.py --display :99 --type "Lin Two"   printable ASCII, one key at a time, Shift as needed
    xkey.py --display :99 --type-file FILE   the same for FILE's first line, so a password typed
                                             into a form never sits on a command line
"""
import argparse
import ctypes
import ctypes.util
import signal
import sys
import time

# The longest hold, in ms: a run's inputs are seconds apart, and none should keep a key down longer.
MAX_HOLD_MS = 60_000
# How long a plain press holds its key, in seconds.
TAP = 0.03


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--display", required=True)
    what = ap.add_mutually_exclusive_group(required=True)
    what.add_argument("--key")
    what.add_argument("--type")
    what.add_argument("--type-file", metavar="FILE")
    ap.add_argument("--hold", type=int, metavar="MS")
    a = ap.parse_args()
    if a.type_file is not None:
        try:
            with open(a.type_file, encoding="ascii") as f:
                a.type = f.readline().rstrip("\r\n")
        except (OSError, UnicodeDecodeError) as e:
            print(f"xkey: --type-file: {e.__class__.__name__}", file=sys.stderr)
            return 2
    if a.type is not None and not (a.type and all(" " <= c <= "~" for c in a.type)):
        print("xkey: --type takes printable ASCII only", file=sys.stderr)
        return 2
    if a.hold is not None and (a.key is None or not 1 <= a.hold <= MAX_HOLD_MS):
        print(f"xkey: --hold takes 1 to {MAX_HOLD_MS} ms, with --key", file=sys.stderr)
        return 2
    held = TAP if a.hold is None else a.hold / 1000
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
    # Every input is checked before any display is opened.
    if a.key is not None:
        sym = x11.XStringToKeysym(a.key.encode())
        if not sym:
            print(f"xkey: no keysym named {a.key!r}", file=sys.stderr)
            return 2
        syms = [sym]
    else:
        # Printable ASCII keysyms equal their character codes (X11 keysymdef.h, Latin 1).
        syms = [ord(c) for c in a.type]
    dpy = x11.XOpenDisplay(a.display.encode())
    if not dpy:
        print(f"xkey: can't open display {a.display}", file=sys.stderr)
        return 1
    try:
        shift = x11.XKeysymToKeycode(dpy, x11.XStringToKeysym(b"Shift_L"))
        # Every keysym is checked against the layout before the first press, so a refused one
        # never leaves half the text typed.
        plan = []
        for sym in syms:
            code = x11.XKeysymToKeycode(dpy, sym)
            if not code:
                print(f"xkey: keysym {sym:#x} has no key on this display", file=sys.stderr)
                return 1
            # Shift when the keysym sits at the key's second level (an upper-case letter, "_", ...).
            # A keysym only a third level (AltGr) reaches can't be typed this way.
            shifted = x11.XkbKeycodeToKeysym(dpy, code, 0, 0) != sym
            if shifted and x11.XkbKeycodeToKeysym(dpy, code, 0, 1) != sym:
                print(f"xkey: keysym {sym:#x} needs more than Shift on this layout", file=sys.stderr)
                return 1
            plan.append((code, shifted))
        # A run that ends during a hold stops it with SIGTERM: the key is released before this ends,
        # so no key stays down on the display for whatever runs there next.
        signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
        for code, shifted in plan:
            if shifted:
                xtst.XTestFakeKeyEvent(dpy, shift, 1, 0)
            xtst.XTestFakeKeyEvent(dpy, code, 1, 0)
            x11.XFlush(dpy)
            try:
                time.sleep(held)
            finally:
                xtst.XTestFakeKeyEvent(dpy, code, 0, 0)
                if shifted:
                    xtst.XTestFakeKeyEvent(dpy, shift, 0, 0)
                x11.XFlush(dpy)
            time.sleep(TAP)
    finally:
        x11.XCloseDisplay(dpy)
    return 0


if __name__ == "__main__":
    sys.exit(main())
