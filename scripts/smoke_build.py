#!/usr/bin/env python3
"""Exercise brief preparation, file creation, verification, and continuation."""
import http.server
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import threading

SOURCE = '''def retrieve(query, documents):
    terms = set(query.lower().split())
    return [{'source': name, 'text': text} for name, text in documents.items()
            if terms.intersection(text.lower().split())]
'''
TESTS = '''import unittest
from retrieval import retrieve
class RetrievalTests(unittest.TestCase):
    def test_sources(self):
        self.assertEqual(retrieve('rust', {'guide.md': 'Rust tools'})[0]['source'], 'guide.md')
    def test_empty(self):
        self.assertEqual(retrieve('missing', {'guide.md': 'Rust tools'}), [])
'''
CHECK = shlex.quote(sys.executable) + ' -m unittest -v'


class Provider(http.server.BaseHTTPRequestHandler):
    requests = []

    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.requests.append(request)
        tools = [m for m in request['messages'] if m['role'] == 'tool']
        calls = []
        if request.get('tools') and not tools:
            calls = [('write_file', {'path': 'retrieval.py', 'content': SOURCE}),
                     ('write_file', {'path': 'test_retrieval.py', 'content': TESTS})]
        elif request.get('tools') and len(tools) == 2:
            calls = [('shell', {'command': CHECK})]
        if calls:
            delta = {'tool_calls': [{'index': i, 'id': f'build-{len(tools)}-{i}', 'type': 'function',
                      'function': {'name': name, 'arguments': json.dumps(args)}}
                     for i, (name, args) in enumerate(calls)]}
            finish = 'tool_calls'
        else:
            delta = {'content': 'BUILD_SLICE_OK'}
            finish = 'stop'
        payload = ('data: ' + json.dumps({'choices': [{'delta': delta, 'finish_reason': finish}]}) +
                   '\n\ndata: [DONE]\n\n').encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def main():
    binary = str(Path(sys.argv[1]).resolve(strict=True))
    with tempfile.TemporaryDirectory(prefix='harness-build-smoke-') as scratch:
        root = Path(scratch)
        env = {k: v for k, v in os.environ.items() if k in ('PATH', 'LANG', 'SYSTEMROOT', 'WINDIR', 'COMSPEC', 'PATHEXT')}
        env.update(HOME=scratch, USERPROFILE=scratch, HARNESS_SKIP_LSP='1')
        def run(*args):
            result = subprocess.run([binary, *args], cwd=root, env=env, stdin=subprocess.DEVNULL,
                                    capture_output=True, text=True, timeout=45)
            assert result.returncode == 0, result.stdout + result.stderr
            return result
        run('build', 'Build a local retrieval backend', '--accept', 'Every result has a source',
            '--check', CHECK, '--prepare-only')
        assert (root / '.harness/build.toml').is_file()
        print('PASS offline build preparation requires no route, credential, or provider call')
        (root / 'AGENTS.md').write_text('Preserve the existing output contract.')
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            (root / '.harness/config.toml').write_text(f'''[router]
default = "smoke"
[providers.smoke]
model = "smoke-model"
base_url = "http://127.0.0.1:{server.server_port}/v1"
[memory]
enabled = false
[ambient]
enabled = false
''')
            result = run('build')
            assert 'BUILD_SLICE_OK' in result.stdout
            assert 'shell' in result.stderr
            assert any('Ran 2 tests' in str(m) and 'OK' in str(m) for r in Provider.requests
                       for m in r['messages'] if m['role'] == 'tool')
            assert (root / 'retrieval.py').is_file()
            assert any('Every result has a source' in str(r['messages'][0]) and
                       'Preserve the existing output contract' in str(r['messages'][0]) for r in Provider.requests)
            print('PASS build brief reaches actual agent; backend files are created and two real tests execute')
            (root / '.harness/BUILD_PROGRESS.md').write_text('NEXT_USEFUL_ACTION: expose the verified retrieval function through an API.')
            Provider.requests.clear()
            run('build')
            assert any('NEXT_USEFUL_ACTION' in str(r['messages'][0]) for r in Provider.requests)
            print('PASS fresh build session reloads the saved outcome and prior progress')
        finally:
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    main()
