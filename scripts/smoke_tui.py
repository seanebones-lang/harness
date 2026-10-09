#!/usr/bin/env python3
"""Exercise the actual terminal UI with a synthetic provider and an isolated home."""
import errno
import http.server
import json
import io
import os
from pathlib import Path
import re
import select
import sqlite3
import subprocess
import sys
import tempfile
import signal
import threading
import time

from smoke_agent import Provider


class CancellationProvider(Provider):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.requests.append(request)
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.end_headers()
        self.wfile.write(('data: ' + json.dumps({'choices': [{'delta': {'content': 'PARTIAL_CANCEL_REPLY'}, 'finish_reason': None}]}) + '\n\n').encode())
        self.wfile.flush()
        time.sleep(3)


class TerminalProvider(Provider):
    def do_POST(self):
        raw = self.rfile.read(int(self.headers['Content-Length']))
        request = json.loads(raw)
        self.rfile = io.BytesIO(raw)
        if any(message.get('role') == 'user' and message.get('content') == 'cancel fixture' for message in request.get('messages', [])):
            return CancellationProvider.do_POST(self)
        return super().do_POST()


def cancellation_smoke(binary):
    provider = http.server.ThreadingHTTPServer(('127.0.0.1', 0), CancellationProvider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='harness-cancel-smoke-') as scratch:
            root = Path(scratch)
            (root / '.harness').mkdir()
            (root / '.harness/config.toml').write_text(f'''[router]
default = "smoke"
[providers.smoke]
model = "smoke-model"
base_url = "http://127.0.0.1:{provider.server_port}/v1"
[memory]
enabled = false
[ambient]
enabled = false
''')
            env = {k: v for k, v in os.environ.items() if k in ('PATH', 'LANG')}
            env.update(HOME=scratch, USERPROFILE=scratch, HARNESS_SKIP_LSP='1')
            with (root / 'output.log').open('w+') as log:
                process = subprocess.Popen([binary, 'run', 'cancel fixture'], cwd=root, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
                try:
                    for _ in range(100):
                        log.flush()
                        if 'PARTIAL_CANCEL_REPLY' in (root / 'output.log').read_text(): break
                        assert process.poll() is None, (root / 'output.log').read_text()
                        time.sleep(.05)
                    else: raise AssertionError('Cancellation response timeout')
                    process.send_signal(signal.SIGINT)
                    assert process.wait(timeout=5) != 0, 'Cancelled one-shot returned success'
                    with sqlite3.connect(root / '.harness/sessions.db') as db:
                        rows = db.execute('SELECT data FROM sessions').fetchall()
                    assert rows and 'PARTIAL_CANCEL_REPLY' in rows[0][0], rows
                    print('PASS Ctrl+C cancellation returns failure and saves the partial response for resume')
                finally:
                    if process.poll() is None: process.kill(); process.wait()
    finally:
        provider.shutdown(); provider.server_close()


def main():
    if os.name != 'posix':
        print('SKIP PTY terminal acceptance: requires a POSIX pseudo-terminal')
        return
    import fcntl
    import pty
    import struct
    import termios

    binary = str(Path(sys.argv[1]).resolve(strict=True))
    cancellation_smoke(binary)
    provider = http.server.ThreadingHTTPServer(('127.0.0.1', 0), TerminalProvider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='harness-tui-smoke-') as scratch:
            root = Path(scratch)
            (root / '.harness').mkdir()
            (root / 'fixture.txt').write_text('TUI_UNICODE_FIXTURE_🦀中é')
            (root / '.harness/config.toml').write_text(f'''[router]
default = "smoke"
[providers.smoke]
model = "smoke-model"
base_url = "http://127.0.0.1:{provider.server_port}/v1"
[memory]
enabled = false
[ambient]
enabled = false
''')
            env = {k: v for k, v in os.environ.items() if k in ('PATH', 'LANG')}
            env.update(HOME=scratch, USERPROFILE=scratch, TERM='xterm-256color', HARNESS_SKIP_LSP='1')
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
            process = subprocess.Popen([binary], cwd=root, env=env, stdin=slave, stdout=slave, stderr=slave)
            os.close(slave)
            output = bytearray()

            def drain(seconds):
                deadline = time.monotonic() + seconds
                while time.monotonic() < deadline:
                    if select.select([master], [], [], .05)[0]:
                        try:
                            chunk = os.read(master, 65536)
                        except OSError as error:
                            if error.errno == errno.EIO:
                                break
                            raise
                        output.extend(chunk)
                        # Answer terminal capability queries if requested.
                        if b'\x1b[6n' in chunk:
                            os.write(master, b'\x1b[1;1R')
                    if process.poll() is not None:
                        break

            try:
                drain(1)
                assert process.poll() is None, output.decode(errors='replace')[-2000:]
                os.write(master, b'\r')  # Dismiss the first-run welcome overlay.
                drain(.2)
                prompt = 'Read fixture.txt and confirm. hé中🦀'
                os.write(master, b'\x1b[200~' + prompt.encode() + b'\x1b[201~\r')
                for _ in range(150):
                    drain(.1)
                    text = re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', output.decode(errors='replace'))
                    if 'HARNESS_SMOKE_OK' in text:
                        break
                    assert process.poll() is None, text[-2000:]
                else:
                    raise AssertionError('Terminal response timeout: ' + text[-2000:])
                # Allow completion to be persisted, then quit through the TUI key handler.
                drain(.5)
                os.write(master, b'\x11')
                drain(5)
                assert process.wait(timeout=5) == 0, output.decode(errors='replace')[-2000:]
                with sqlite3.connect(root / '.harness/sessions.db') as db:
                    rows = db.execute('SELECT data FROM sessions').fetchall()
                assert rows, 'No terminal session persisted'
                data = json.loads(rows[-1][0])
                serialized = json.dumps(data, ensure_ascii=False)
                assert prompt in serialized and 'HARNESS_SMOKE_OK' in serialized, serialized
                print('PASS terminal startup, Unicode input, streamed tool round trip, session persistence, clean quit')
                output.clear()
                master2, slave2 = pty.openpty()
                fcntl.ioctl(slave2, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
                os.close(master)
                master = master2
                process = subprocess.Popen([binary], cwd=root, env=env, stdin=slave2, stdout=slave2, stderr=slave2)
                os.close(slave2)
                drain(1)
                os.write(master, b'\x1b[200~cancel fixture\x1b[201~\r')
                for _ in range(100):
                    drain(.05)
                    text = re.sub(r'\x1b\[[0-?]*[ -/]*[@-~]', '', output.decode(errors='replace'))
                    if 'PARTIAL_CANCEL_REPLY' in text: break
                    assert process.poll() is None, text[-2000:]
                else: raise AssertionError('Terminal cancellation response timeout: ' + text[-2000:])
                os.write(master, b'\x11')
                drain(5)
                assert process.wait(timeout=5) == 0, 'Terminal cancellation did not quit cleanly'
                with sqlite3.connect(root / '.harness/sessions.db') as db:
                    rows = db.execute('SELECT data FROM sessions').fetchall()
                assert any('PARTIAL_CANCEL_REPLY' in row[0] for row in rows), 'Cancelled terminal response was lost'
                print('PASS terminal quit cancels the active provider and persists its partial response')
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                os.close(master)
    finally:
        provider.shutdown()
        provider.server_close()


if __name__ == '__main__':
    main()
