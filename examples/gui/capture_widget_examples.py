#!/usr/bin/env python3
"""Refresh book screenshots from built widget examples on a private X server."""
from pathlib import Path
import sys
import time
import tomllib

from PIL import Image
from Xlib import X, XK
from Xlib.ext import xtest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'experiments/packages/egui/tests'))
from smoke_x11 import private_display, running, wait_for


def capture(server, binary, title, png, logs):
    root = server.screen().root
    with running([binary], logs / f'{binary.name}.log') as app:
        def window():
            return next((w for w in root.query_tree().children if w.get_wm_name() == title
                         and w.get_attributes().map_state == X.IsViewable), None)
        win = wait_for(window, app)
        if binary.name == 'tooltip':
            point = root.translate_coords(win, 38, 34)
            xtest.fake_input(server, X.MotionNotify, x=point.x, y=point.y)
            server.sync()
            time.sleep(1)

        def painted():
            g = win.get_geometry()
            frame = win.get_image(0, 0, g.width, g.height, X.ZPixmap, 0xffffffff)
            image = Image.frombytes('RGB', (g.width, g.height), frame.data, 'raw', 'BGRX')
            return image if image.getextrema() != tuple((c, c) for c in image.getpixel((0, 0))) else None

        image = wait_for(painted, app)
        if binary.name == 'completion':
            win.set_input_focus(X.RevertToParent, X.CurrentTime)
            point = root.translate_coords(win, 45, 38)
            for event, detail in [(X.MotionNotify, 0), (X.ButtonPress, 1), (X.ButtonRelease, 1)]:
                xtest.fake_input(server, event, detail, x=point.x, y=point.y)
                server.sync()
                time.sleep(0.1)
            for char in 'ap':
                code = server.keysym_to_keycode(XK.string_to_keysym(char))
                for event in [X.KeyPress, X.KeyRelease]:
                    xtest.fake_input(server, event, code)
                    server.sync()
                    time.sleep(0.1)
            time.sleep(0.3)
            image = wait_for(painted, app)
        png.parent.mkdir(parents=True, exist_ok=True)
        image.save(png, optimize=True)
        print(png.relative_to(ROOT), flush=True)


def main():
    logs = ROOT / 'target/widget-screenshots'
    logs.mkdir(parents=True, exist_ok=True)
    with private_display(logs) as server:
        for backend in ('egui', 'iced'):
            package = ROOT / f'examples/gui/{backend}_widgets'
            pages = ROOT / f'book/src/design/stdlib/gui-{backend}-widgets'
            for entry in tomllib.loads((package / 'plenty.toml').read_text())['bin']:
                name = entry['name']
                capture(server, package / 'target/optimized' / name, f'Plenty {backend}',
                        pages / 'screenshots' / f'{name}.png', logs)


if __name__ == '__main__':
    main()
