#!/usr/bin/env python3
"""Exercise real winit/X11 widgets, exclusively on a new private headless compositor."""
import argparse
from contextlib import contextmanager
import os
from pathlib import Path
import secrets
import select
import subprocess
import tempfile
import time

from PIL import Image
from Xlib import X, display, protocol
from Xlib.ext import xtest

PACKAGE = Path(__file__).resolve().parents[1]


@contextmanager
def running(command, logfile, **kwargs):
    with logfile.open("wb") as log:
        process = subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT, **kwargs)
        try:
            yield process
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()


def wait_for(probe, process):
    end = time.monotonic() + 25
    while time.monotonic() < end and process.poll() is None:
        result = probe()
        if result:
            return result
        time.sleep(0.1)
    raise AssertionError(f"timed out or process exited: {process.args} ({process.poll()})")


def exercise(server, binary, out, expect_render_error):
    screen = server.screen()
    with running([binary], out / "runner.log") as process:
        def window():
            for child in screen.root.query_tree().children:
                if child.get_wm_name() == 'Plenty iced' and child.get_attributes().map_state == X.IsViewable:
                    return child
        win = wait_for(window, process)
        win.set_input_focus(X.RevertToParent, X.CurrentTime)
        server.sync()

        def capture():
            frame = win.get_image(0, 0, 480, 280, X.ZPixmap, 0xffffffff)
            return Image.frombytes("RGB", (480, 280), frame.data, "raw", "BGRX")

        def count_pixels(img):
            return img.crop((24, 55, 130, 81)).tobytes()

        def ready(predicate):
            img = capture()
            return img if predicate(count_pixels(img)) else None

        before = wait_for(lambda: ready(lambda crop: len(set(crop)) > 8), process)
        before.save(out / 'initial.png')
        prior = count_pixels(before)
        for label, x in [('one', 60), ('two', 60), ('reset', 151)]:
            # Translate from client to private root coordinates for real XTest input.
            point = screen.root.translate_coords(win, x, 105)
            xtest.fake_input(server, X.MotionNotify, x=point.x, y=point.y)
            server.sync()
            time.sleep(0.1)
            xtest.fake_input(server, X.ButtonPress, 1)
            server.sync()
            time.sleep(0.1)
            xtest.fake_input(server, X.ButtonRelease, 1)
            server.sync()
            if expect_render_error:
                assert process.wait(timeout=15) == 1, 'render error did not stop the GUI'
                assert 'main returned iced.IcedError.LimitExceeded' in (out / 'runner.log').read_text()
                return
            current = wait_for(lambda: ready(lambda crop: crop != prior), process)
            current.save(out / f'{label}.png')
            prior = count_pixels(current)
            assert (prior == count_pixels(before)) == (label == 'reset')
        close = protocol.event.ClientMessage(window=win, client_type=server.intern_atom('WM_PROTOCOLS'),
                                             data=(32, [server.intern_atom('WM_DELETE_WINDOW'), 0, 0, 0, 0]))
        win.send_event(close)
        server.flush()
        assert process.wait(timeout=15) == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=PACKAGE / 'target/smoke')
    parser.add_argument('--binary', type=Path, help='exercise a package-built executable instead')
    parser.add_argument('--fast-compile', action='store_true', help='exercise the fast-compile executable')
    parser.add_argument('--expect-render-error', action='store_true',
                        help='instrumented fixture only: first press must return LimitExceeded to main')
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    # Never probe a user's desktop, even if setup fails. The X display number
    # comes from our own new Xwayland, not a guessed number or inherited env.
    for key in ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'XAUTHORITY']:
        os.environ.pop(key, None)
    with tempfile.TemporaryDirectory(prefix='iced-display-') as runtime:
        auth = Path(runtime) / 'auth'
        auth.touch(mode=0o600)
        os.environ.update(XDG_RUNTIME_DIR=runtime, WAYLAND_DISPLAY='iced-private', XAUTHORITY=str(auth))
        cookie = secrets.token_hex(16)
        def authorize(number):
            subprocess.run(['xauth', 'add', f':{number}', '.', cookie], check=True)
        authorize(0)
        with running(['weston', '--no-config', '--backend=headless', '--renderer=pixman',
                      '--shell=kiosk-shell.so', '--idle-time=0', '--socket=iced-private'], args.out / 'weston.log') as compositor:
            wait_for((Path(runtime) / 'iced-private').is_socket, compositor)
            read, write = os.pipe()
            with os.fdopen(read) as result, running(['Xwayland', '-displayfd', str(write), '-auth', str(auth),
                         '-geometry', '800x600', '-nolisten', 'tcp'], args.out / 'xwayland.log', pass_fds=(write,)):
                os.close(write)
                assert select.select([result], [], [], 20)[0], 'private Xwayland not ready'
                number = int(result.readline())
                authorize(number)
                del os.environ['WAYLAND_DISPLAY']
                os.environ['DISPLAY'] = f':{number}'
                server = display.Display(os.environ['DISPLAY'])
                try:
                    profile = 'fast-compile' if args.fast_compile else 'optimized'
                    binary = args.binary or PACKAGE / 'target' / profile / 'plenty-iced'
                    exercise(server, binary.resolve(strict=True), args.out, args.expect_render_error)
                finally:
                    server.close()
    result = 'render error returned to main' if args.expect_render_error else 'two increments, reset and native close'
    print(f'PASS: private X11: {result}; {args.out}')


if __name__ == '__main__':
    main()
