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

Before each key press, a pointer over no window is moved into the client's window (#244): a nested
display has no window manager, so a press goes to the window under the pointer, and a client in the
world leaves the pointer on the display's bottom row, outside its window, where its presses were
lost (Task 13's m4-s3 and m4-s4). A click to bring it back acts on the game's UI too (it took the
title screen in two runs). The move is the shortest into the display's largest window, and best
effort: if it can't be made, the key is pressed as before.
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
# How long a moved pointer settles before the key press, in seconds: the window gets its enter and
# focus events first.
SETTLE = 0.05
IS_VIEWABLE, INPUT_OUTPUT = 2, 1  # XWindowAttributes' map_state and class


def pointer_target(pointer, over_window, windows):
    """Where to move the pointer before a key press, or None to leave it: when it's over no window,
    the nearest point inside the largest window (the topmost of equals), so that the key goes to
    that window. `windows` are the display's viewable top-level windows, bottom first, each the
    (x, y, width, height) of its inside."""
    if over_window or not windows:
        return None
    x, y, w, h = max(reversed(windows), key=lambda r: r[2] * r[3])
    px, py = pointer
    return min(max(px, x), x + w - 1), min(max(py, y), y + h - 1)


class XWindowAttributes(ctypes.Structure):
    """Xlib's XWindowAttributes (X11/Xlib.h), read only up to the fields used here."""
    _fields_ = [("x", ctypes.c_int), ("y", ctypes.c_int), ("width", ctypes.c_int), ("height", ctypes.c_int),
                ("border_width", ctypes.c_int), ("depth", ctypes.c_int), ("visual", ctypes.c_void_p),
                ("root", ctypes.c_ulong), ("c_class", ctypes.c_int), ("bit_gravity", ctypes.c_int),
                ("win_gravity", ctypes.c_int), ("backing_store", ctypes.c_int), ("backing_planes", ctypes.c_ulong),
                ("backing_pixel", ctypes.c_ulong), ("save_under", ctypes.c_int), ("colormap", ctypes.c_ulong),
                ("map_installed", ctypes.c_int), ("map_state", ctypes.c_int), ("all_event_masks", ctypes.c_long),
                ("your_event_mask", ctypes.c_long), ("do_not_propagate_mask", ctypes.c_long),
                ("override_redirect", ctypes.c_int), ("screen", ctypes.c_void_p)]


# Xlib's default error handler ends the process. While the windows are listed, a window that goes
# away between the listing and its attributes is an error to skip instead. Kept at module level, so
# the callback is never freed.
_IGNORE_X_ERRORS = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p)(lambda dpy, event: 0)


class Pointer:
    """Moves the pointer into the client's window before a key press (pointer_target), through XTEST."""

    def __init__(self, x11, xtst, dpy):
        self.x11, self.xtst, self.dpy = x11, xtst, dpy
        win = ctypes.c_ulong
        x11.XSetErrorHandler.restype = ctypes.c_void_p
        x11.XSetErrorHandler.argtypes = [ctypes.c_void_p]
        x11.XDefaultRootWindow.restype = win
        x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        x11.XQueryPointer.argtypes = [ctypes.c_void_p, win] + [ctypes.POINTER(win)] * 2 + \
            [ctypes.POINTER(ctypes.c_int)] * 4 + [ctypes.POINTER(ctypes.c_uint)]
        x11.XQueryTree.argtypes = [ctypes.c_void_p, win, ctypes.POINTER(win), ctypes.POINTER(win),
                                   ctypes.POINTER(ctypes.POINTER(win)), ctypes.POINTER(ctypes.c_uint)]
        x11.XGetWindowAttributes.argtypes = [ctypes.c_void_p, win, ctypes.c_void_p]
        x11.XFree.argtypes = [ctypes.c_void_p]
        xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
        self.root = x11.XDefaultRootWindow(dpy)

    def windows(self):
        """The viewable top-level windows, bottom first, as pointer_target takes them."""
        root, parent, n = ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_uint()
        children = ctypes.POINTER(ctypes.c_ulong)()
        if not self.x11.XQueryTree(self.dpy, self.root, ctypes.byref(root), ctypes.byref(parent),
                                   ctypes.byref(children), ctypes.byref(n)):
            return []
        try:
            ids = [children[i] for i in range(n.value)]
        finally:
            if children:
                self.x11.XFree(children)
        out = []
        xlibs = self.x11.XSetErrorHandler(ctypes.cast(_IGNORE_X_ERRORS, ctypes.c_void_p))
        try:
            for w in ids:
                buf = (ctypes.c_char * 512)()  # room to spare beyond XWindowAttributes, whatever its size
                if not self.x11.XGetWindowAttributes(self.dpy, w, buf):
                    continue  # gone since the listing
                a = XWindowAttributes.from_buffer(buf)
                if a.map_state == IS_VIEWABLE and a.c_class == INPUT_OUTPUT:
                    out.append((a.x + a.border_width, a.y + a.border_width, a.width, a.height))
        finally:
            self.x11.XSetErrorHandler(xlibs)
        return out

    def place(self):
        """Moves the pointer into the client's window if it's over no window; returns where to."""
        root, child = ctypes.c_ulong(), ctypes.c_ulong()
        rx, ry, wx, wy, mask = ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_int(), ctypes.c_uint()
        if not self.x11.XQueryPointer(self.dpy, self.root, ctypes.byref(root), ctypes.byref(child), ctypes.byref(rx),
                                      ctypes.byref(ry), ctypes.byref(wx), ctypes.byref(wy), ctypes.byref(mask)):
            return None  # on another screen
        to = None if child.value else pointer_target((rx.value, ry.value), False, self.windows())
        if to:
            self.xtst.XTestFakeMotionEvent(self.dpy, -1, to[0], to[1], 0)
            self.x11.XFlush(self.dpy)
        return to


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
        pointer = None
        try:
            pointer = Pointer(x11, xtst, dpy)
        except (AttributeError, OSError, ctypes.ArgumentError) as e:
            print(f"xkey: the pointer can't be placed ({e.__class__.__name__}); pressing as it lies", file=sys.stderr)
        for code, shifted in plan:
            try:
                if pointer and pointer.place():
                    time.sleep(SETTLE)
            except (OSError, ctypes.ArgumentError, ValueError) as e:
                print(f"xkey: the pointer wasn't placed ({e.__class__.__name__}); pressing as it lies", file=sys.stderr)
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
