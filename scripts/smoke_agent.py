#!/usr/bin/env python3
"""Deterministic real-binary agent/HTTP round trip against a local synthetic provider."""
import http.server
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request


class Provider(http.server.BaseHTTPRequestHandler):
    requests = []

    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.requests.append(request)
        messages = request.get('messages', [])
        has_tool = any(m.get('role') == 'tool' for m in messages)
        if request.get('tools') and not has_tool:
            delta = {'tool_calls': [{'index': 0, 'id': 'smoke-read', 'type': 'function',
                      'function': {'name': 'read_file', 'arguments': '{"path":"fixture.txt"}'}}]}
            finish = 'tool_calls'
        else:
            delta = {'content': 'HARNESS_SMOKE_OK'}
            finish = 'stop'
        if request.get('stream'):
            data = ('data: ' + json.dumps({'choices': [{'index': 0, 'delta': delta, 'finish_reason': None}]}) +
                    '\n\ndata: ' + json.dumps({'choices': [{'index': 0, 'delta': {}, 'finish_reason': finish}]}) +
                    '\n\ndata: [DONE]\n\n').encode()
            content_type = 'text/event-stream'
        else:
            data = json.dumps({'choices': [{'message': {'role': 'assistant', 'content': 'Smoke session'},
                                          'finish_reason': 'stop'}]}).encode()
            content_type = 'application/json'
        self.send_response(200)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)


def main():
    binary = str(Path(sys.argv[1]).resolve(strict=True))
    provider = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    threading.Thread(target=provider.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='harness-agent-smoke-') as scratch:
            root = Path(scratch)
            (root / '.harness').mkdir()
            (root / 'fixture.txt').write_text('SYNTHETIC_FIXTURE_CONTENT')
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
            env = {k: v for k, v in os.environ.items() if k in
                   ('PATH', 'SYSTEMROOT', 'WINDIR', 'COMSPEC', 'PATHEXT', 'LANG', 'TERM')}
            env.update(HOME=scratch, USERPROFILE=scratch, HARNESS_SKIP_LSP='1')
            result = subprocess.run([binary, 'run', 'Read fixture.txt and confirm.'], cwd=root, env=env,
                                    stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=45)
            assert result.returncode == 0, result.stderr
            assert 'HARNESS_SMOKE_OK' in result.stdout, result.stdout
            assert any('SYNTHETIC_FIXTURE_CONTENT' in str(m) for r in Provider.requests
                       for m in r.get('messages', []) if m.get('role') == 'tool'), Provider.requests
            print('PASS CLI provider streaming, read_file execution, tool-result continuation')
            with socket.socket() as reserve:
                reserve.bind(('127.0.0.1', 0))
                port = reserve.getsockname()[1]
            base = f'http://127.0.0.1:{port}'
            with (root / 'server.log').open('w+') as log:
                server = subprocess.Popen([binary, 'serve', '--addr', f'127.0.0.1:{port}'],
                                          cwd=root, env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
                try:
                    for _ in range(100):
                        if server.poll() is not None:
                            log.seek(0)
                            raise RuntimeError(log.read())
                        try:
                            health = json.load(urllib.request.urlopen(base + '/api/health', timeout=1))
                            break
                        except (urllib.error.URLError, TimeoutError):
                            time.sleep(.1)
                    else:
                        raise RuntimeError('HTTP readiness timeout')
                    try:
                        urllib.request.urlopen(base + '/api/sessions', timeout=5)
                        raise AssertionError('Unauthenticated sessions accepted')
                    except urllib.error.HTTPError as error:
                        assert error.code == 401, error.code
                    headers = {'Authorization': 'Bearer ' + health['auth_token'], 'Content-Type': 'application/json'}
                    request = urllib.request.Request(base + '/api/chat',
                        json.dumps({'prompt': 'Read fixture.txt and confirm.'}).encode(), headers)
                    with urllib.request.urlopen(request, timeout=30) as response:
                        events = response.read().decode()
                    assert 'HARNESS_SMOKE_OK' in events, events
                    sessions = json.load(urllib.request.urlopen(urllib.request.Request(
                        base + '/api/sessions', headers=headers), timeout=5))
                    assert sessions, 'No persisted sessions'
                    exported = subprocess.run([binary, 'export', sessions[0]['id']], cwd=root, env=env,
                        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15)
                    assert exported.returncode == 0, exported.stderr
                    assert 'HARNESS_SMOKE_OK' in exported.stdout, exported.stdout
                    print('PASS HTTP auth, chat SSE, persistent sessions, Markdown export')
                finally:
                    server.terminate()
                    try:
                        server.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        server.kill()
                        server.wait()
            assert all(r['model'] == 'smoke-model' for r in Provider.requests)
            print('PASS exact model route preserved at provider HTTP boundary')
    finally:
        provider.shutdown()
        provider.server_close()


if __name__ == '__main__':
    main()
