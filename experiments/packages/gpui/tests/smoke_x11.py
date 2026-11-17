#!/usr/bin/env python3
"""Real X11 renderer/input/close smoke; never substitutes a GPUI test platform.

Requires a running X11/Xwayland desktop, libX11, libXtst and ImageMagick `import`.
Screenshots/logs are retained under --out (default /tmp/plenty-gpui-smoke).
"""
import argparse
import ctypes as C
import ctypes.util
import os
from pathlib import Path
import subprocess
import time

PACKAGE = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("/tmp/plenty-gpui-smoke"))
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    x = C.CDLL(ctypes.util.find_library("X11"))
    xt = C.CDLL(ctypes.util.find_library("Xtst"))
    display_type, window_type = C.c_void_p, C.c_ulong
    x.XOpenDisplay.argtypes = [C.c_char_p]
    x.XOpenDisplay.restype = display_type
    x.XDefaultRootWindow.argtypes = [display_type]
    x.XDefaultRootWindow.restype = window_type
    x.XQueryTree.argtypes = [display_type, window_type, C.POINTER(window_type), C.POINTER(window_type), C.POINTER(C.POINTER(window_type)), C.POINTER(C.c_uint)]
    x.XFetchName.argtypes = [display_type, window_type, C.POINTER(C.c_void_p)]
    x.XFree.argtypes = [C.c_void_p]
    x.XTranslateCoordinates.argtypes = [display_type, window_type, window_type, C.c_int, C.c_int, C.POINTER(C.c_int), C.POINTER(C.c_int), C.POINTER(window_type)]
    x.XRaiseWindow.argtypes = [display_type, window_type]
    x.XSetInputFocus.argtypes = [display_type, window_type, C.c_int, C.c_ulong]
    x.XFlush.argtypes = [display_type]
    x.XCloseDisplay.argtypes = [display_type]
    x.XKeysymToKeycode.argtypes = [display_type, C.c_ulong]
    x.XKeysymToKeycode.restype = C.c_uint
    xt.XTestFakeMotionEvent.argtypes = [display_type, C.c_int, C.c_int, C.c_int, C.c_ulong]
    xt.XTestFakeButtonEvent.argtypes = [display_type, C.c_uint, C.c_int, C.c_ulong]
    xt.XTestFakeKeyEvent.argtypes = [display_type, C.c_uint, C.c_int, C.c_ulong]
    display = x.XOpenDisplay(None)
    assert display, "X11 display unavailable; this smoke cannot run headlessly"
    root = x.XDefaultRootWindow(display)

    def find(parent):
        actual_root, actual_parent = window_type(), window_type()
        children, count = C.POINTER(window_type)(), C.c_uint()
        if not x.XQueryTree(display, parent, C.byref(actual_root), C.byref(actual_parent), C.byref(children), C.byref(count)):
            return None
        values = list(children[:count.value])
        if children:
            x.XFree(children)
        for window in values:
            name = C.c_void_p()
            if x.XFetchName(display, window, C.byref(name)) and name:
                text = C.string_at(name)
                x.XFree(name)
                if text == b"Plenty GPUI":
                    return window
            found = find(window)
            if found:
                return found
        return None

    log = (args.out / "runner.log").open("w")
    env = {key: value for key, value in os.environ.items() if key != "WAYLAND_DISPLAY"}
    process = subprocess.Popen([str(PACKAGE / "target/optimized/plenty-gpui")], env=env, stdout=log, stderr=log)
    try:
        window = None
        deadline = time.monotonic() + 40
        while time.monotonic() < deadline and process.poll() is None:
            window = find(root)
            if window:
                break
            time.sleep(0.2)
        assert window, f"window not opened; inspect {log.name}"
        x.XRaiseWindow(display, window)
        x.XSetInputFocus(display, window, 1, 0)
        x.XFlush(display)
        time.sleep(2)

        def capture(name):
            subprocess.run(["import", "-window", str(window), str(args.out / f"{name}.png")], check=True, timeout=10)
            return subprocess.check_output(["import", "-window", str(window), "-crop", "200x38+45+132", "rgb:-"], timeout=10)

        def click(local_x, local_y):
            screen_x, screen_y, child = C.c_int(), C.c_int(), window_type()
            assert x.XTranslateCoordinates(display, window, root, local_x, local_y, C.byref(screen_x), C.byref(screen_y), C.byref(child))
            xt.XTestFakeMotionEvent(display, -1, screen_x.value, screen_y.value, 0)
            x.XFlush(display)
            time.sleep(0.2)
            assert xt.XTestFakeButtonEvent(display, 1, 1, 0)
            x.XFlush(display)
            time.sleep(0.1)
            assert xt.XTestFakeButtonEvent(display, 1, 0, 0)
            x.XFlush(display)
            time.sleep(1)

        # GPUI Linux client-side decorations add shadow/titlebar space at
        # desktop scale 1. This fixture uses the counter's fixed default layout.
        screen_x, screen_y, child = C.c_int(), C.c_int(), window_type()
        x.XTranslateCoordinates(display, window, root, 300, 280, C.byref(screen_x), C.byref(screen_y), C.byref(child))
        xt.XTestFakeMotionEvent(display, -1, screen_x.value, screen_y.value, 0)
        x.XFlush(display)
        deadline = time.monotonic() + 20
        while True:
            before = capture("before")
            if len(set(before)) > 8:
                break
            assert time.monotonic() < deadline, "text pixels were never rendered"
            time.sleep(0.5)
        # Some desktop policies consume the first click to focus the window.
        # Retry boundedly, but require actual changed/restored renderer pixels.
        for _ in range(3):
            click(105, 215)
            after = capture("incremented")
            if before != after:
                break
        assert before != after, "increment did not change rendered Count pixels"
        for _ in range(3):
            click(210, 215)
            reset = capture("reset")
            if before == reset:
                break
        assert before == reset, "reset did not restore rendered Count pixels"
        alt, f4 = x.XKeysymToKeycode(display, 0xffe9), x.XKeysymToKeycode(display, 0xffc1)
        for code, pressed in [(alt, 1), (f4, 1), (f4, 0), (alt, 0)]:
            xt.XTestFakeKeyEvent(display, code, pressed, 0)
        x.XFlush(display)
        assert process.wait(timeout=15) == 0, f"unclean close; inspect {log.name}"
        print(f"PASS: real X11 window, rendered text/layout, click/increment/reset, close; artifacts {args.out}")
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
        log.close()
        x.XCloseDisplay(display)


if __name__ == "__main__":
    main()
