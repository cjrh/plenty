#!/usr/bin/env python3
"""Qualify the command picker with native XTest input on a fresh private display."""
import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

from Xlib import X, XK, protocol
from Xlib.ext import xtest

EXAMPLES = Path(__file__).resolve().parent
sys.path.insert(0, str(EXAMPLES.parents[1] / 'experiments/packages/egui/tests'))
from smoke_x11 import private_display, running, wait_for


def exercise(server, exe, out):
    log = out / 'runner.log'
    with running([exe], log) as proc:
        root = server.screen().root
        def window():
            return next((w for w in root.query_tree().children if w.get_wm_name() == 'Plenty egui'
                         and w.get_attributes().map_state == X.IsViewable), None)
        win = wait_for(window, proc)
        win.set_input_focus(X.RevertToParent, X.CurrentTime)
        server.sync()
        wait_for(lambda: 'QUERY f猫lder' in log.read_text(), proc)
        point = root.translate_coords(win, 45, 38)
        for event, detail in [(X.MotionNotify, 0), (X.ButtonPress, 1), (X.ButtonRelease, 1)]:
            xtest.fake_input(server, event, detail, x=point.x, y=point.y)
            server.sync()
            time.sleep(0.12)

        def key(symbol, down=True, up=True):
            code = server.keysym_to_keycode(XK.string_to_keysym(symbol))
            assert code, symbol
            for event, send in [(X.KeyPress, down), (X.KeyRelease, up)]:
                if send:
                    xtest.fake_input(server, event, code)
                    server.sync()
                    time.sleep(0.08)
        key('Home')
        key('Right')
        key('Shift_L', up=False)
        key('Right')
        key('Shift_L', down=False)
        # Idle repaints must neither erase the selection nor steal focus.
        time.sleep(0.7)
        key('o')
        wait_for(lambda: 'QUERY folder\nMATCH Open folder\n' in log.read_text(), proc)
        key('Return')
        wait_for(lambda: 'SELECT Open folder' in log.read_text(), proc)
        # Make the window repaint following the key release, without another edit.
        win.clear_area(exposures=True)
        server.sync()
        time.sleep(0.7)
        close = protocol.event.ClientMessage(window=win, client_type=server.intern_atom('WM_PROTOCOLS'),
                                             data=(32, [server.intern_atom('WM_DELETE_WINDOW'), 0, 0, 0, 0]))
        win.send_event(close)
        server.flush()
        assert proc.wait(timeout=15) == 0
    lines = log.read_text().splitlines()
    assert [s for s in lines if s.startswith('MATCH ')] == ['MATCH Open folder'] * lines.count('QUERY folder')
    assert [s for s in lines if s.startswith('SELECT ')] == ['SELECT Open folder']
    assert [s for s in lines if s.startswith('DROP ')] == ['DROP folder Open folder']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plenty', type=Path, required=True)
    args = parser.parse_args()
    compiler = args.plenty.resolve(strict=True)
    out = EXAMPLES.parents[1] / 'target/egui-picker-check'
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp, private_display(out) as server:
        pkg = Path(tmp)
        example = EXAMPLES / 'egui_command_picker'
        for name in ('plenty.toml', 'plenty.lock'):
            shutil.copy(example / name, pkg / name)
        shutil.copytree(example / 'src', pkg / 'src')
        source = pkg / 'src/main.plenty'
        probe = source.read_text().replace('self.query = ""', 'self.query = "f猫lder"').replace(
            '    def __init__', '    def __del__(self) -> ():\n        drop(print("DROP", self.query, self.selected))\n    def __init__').replace(
            '    for command', '    drop(print("QUERY", state.query))\n    for command').replace(
            '            ui.label(command)?', '            ui.label(command)?\n            drop(print("MATCH", *command))').replace(
            '                select = False', '                select = False\n                drop(print("SELECT", state.selected))')
        source.write_text(probe)
        for profile in ('optimized', 'fast-compile'):
            args = [] if profile == 'optimized' else ['--fast-compile']
            subprocess.run([compiler, 'build', '--locked', *args], cwd=pkg, check=True)
            dest = out / profile
            dest.mkdir(exist_ok=True)
            exercise(server, pkg / 'target' / profile / 'egui_command_picker', dest)
            print(f'{profile}: f猫lder → folder, only Open folder matched, one Enter selection, one owner drop')


if __name__ == '__main__':
    main()
