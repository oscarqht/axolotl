#!/usr/bin/env python3
"""Drive native keyboard smoke via System Events (Accessibility required)."""
import json
import selectors
import subprocess
import sys
import time

process = subprocess.Popen([sys.argv[1], '--smoke-test', '--smoke-keyboard', '--smoke-keyboard-os'],
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
selector = selectors.DefaultSelector()
selector.register(process.stdout, selectors.EVENT_READ)
pending = b''
passed = False
try:
    deadline = time.monotonic() + 600
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
                if line.startswith('AXOLOTL_SMOKE '):
                    passed = json.loads(line.removeprefix('AXOLOTL_SMOKE '))['result'] == 'passed'
                if not line.startswith('AXOLOTL_KEYBOARD '):
                    continue
                request = json.loads(line.removeprefix('AXOLOTL_KEYBOARD '))
                assert request['pid'] == process.pid
                modifiers = request['modifiers']
                assert all(m in ['shift', 'option', 'command', 'control'] for m in modifiers)
                using = ' using {' + ', '.join(m + ' down' for m in modifiers) + '}' if modifiers else ''
                code = int(request['key_code'])
                script = f'''tell application "System Events"
                    set appProcess to first application process whose unix id is {process.pid}
                    set frontmost of appProcess to true
                    delay 0.5
                    set activePid to unix id of (first application process whose frontmost is true)
                    if activePid is not {process.pid} then error "Cannot activate test app"
                    key code {code}{using}
                end tell'''
                subprocess.run(['osascript', '-e', script], check=True, timeout=10)
        if process.poll() is not None and not selector.get_map():
            assert process.returncode == 0 and passed, f'Keyboard smoke failed ({process.returncode})'
            break
    else:
        raise TimeoutError('Native keyboard scenario did not finish')
finally:
    if process.poll() is None:
        process.terminate()
        process.wait(timeout=10)
    selector.close()
