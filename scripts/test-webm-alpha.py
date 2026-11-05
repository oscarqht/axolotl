#!/usr/bin/env python3
"""Launch Axolotl's real WKWebView regression against a localhost fixture."""
import functools
import http.server
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading

root = Path(__file__).resolve().parents[1]
fixture = root / 'crates/desktop/assets/video-fixture'
class Handler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass

with http.server.ThreadingHTTPServer(('127.0.0.1', 0), functools.partial(Handler, directory=str(fixture))) as server:
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        binary = str(Path(sys.argv[1]).resolve())
        output = Path(sys.argv[2] if len(sys.argv) > 2 else '/tmp/axolotl-video-validation')
        output.mkdir(parents=True, exist_ok=True)
        for baseline in (False, True):
            with tempfile.TemporaryDirectory(prefix='axolotl-video-') as profile:
                env = {**os.environ, 'AXOLOTL_DATA_DIR': profile,
                       'AXOLOTL_VIDEO_TEST_URL': f'http://127.0.0.1:{server.server_port}/',
                       'AXOLOTL_VIDEO_SNAPSHOT': str(Path(profile) / 'snapshot.png')}
                args = [binary, '--smoke-test', '--smoke-video']
                if baseline:
                    args.append('--smoke-video-baseline')
                result = subprocess.run(args, env=env, capture_output=True, text=True, timeout=90)
                print(result.stdout)
                if result.returncode:
                    print(result.stderr, file=sys.stderr)
                    raise SystemExit(result.returncode)
                snapshot = Path(profile) / 'snapshot.png'
                if not snapshot.is_file() or snapshot.stat().st_size == 0:
                    raise RuntimeError('Native WKWebView snapshot was not written')
                shutil.copyfile(snapshot, output / ('before.png' if baseline else 'after.png'))
    finally:
        server.shutdown()
