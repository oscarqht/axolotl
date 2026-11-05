#!/usr/bin/env python3
"""Run the smoke-enabled binary's native link scenario, then send Cmd+Q.
Requires macOS GUI and System Events Accessibility/Automation permission.
Usage: python3 scripts/test-macos-input.py /absolute/path/to/axolotl
Clicks use AppKit hit testing in the app; only Cmd+Q uses OS keyboard input.
"""
import json
import selectors
import subprocess
import sys
import time


def main():
    process = subprocess.Popen([sys.argv[1], '--smoke-test', '--smoke-input'],
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    pending = b''
    requested = False
    deadline = time.monotonic() + 90
    try:
        while time.monotonic() < deadline:
            for key, _ in selector.select(timeout=0.2):
                chunk = key.fileobj.read1(65536)
                if not chunk:
                    selector.unregister(key.fileobj)
                    continue
                pending += chunk
                while b'\n' in pending:
                    line, pending = pending.split(b'\n', 1)
                    line = line.decode()
                    print(line, flush=True)
                    if not line.startswith('AXOLOTL_INPUT '):
                        continue
                    request = json.loads(line.removeprefix('AXOLOTL_INPUT '))
                    assert request == {'action': 'quit', 'pid': process.pid}
                    script = f'''tell application "System Events"
                        set appProcess to first application process whose unix id is {process.pid}
                        set frontmost of appProcess to true
                        delay 0.3
                        set activePid to unix id of (first application process whose frontmost is true)
                        if activePid is not {process.pid} then error "Cannot activate test app"
                        keystroke "q" using command down
                    end tell'''
                    subprocess.run(['osascript', '-e', script], check=True, timeout=10)
                    requested = True
            if process.poll() is not None:
                assert process.returncode == 0, f'App exited {process.returncode}'
                assert requested, 'App exited before quit verification'
                time.sleep(3)
                processes = subprocess.check_output(['ps', '-axo', 'command='], text=True).splitlines()
                assert not any(line.strip().split(' --smoke-test')[0] == sys.argv[1] for line in processes), 'App relaunched'
                print('PASS: native sibling links, popup/ordinary clicks, focused Cmd+Q; no relaunch after 3 seconds')
                return
        raise TimeoutError('Native input scenario did not finish')
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
        selector.close()


if __name__ == '__main__':
    main()
