#!/usr/bin/env python3
"""Real binary: selected workspace, skill loading, Office read, research artifacts, continuation."""
import http.server
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import threading
from test_document_extract import office


class Provider(http.server.BaseHTTPRequestHandler):
    requests = []
    phase = 'documents'

    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        self.requests.append(request)
        results = [m for m in request['messages'] if m['role'] == 'tool']
        calls = []
        if not results:
            calls = [('read_file', {'path': f'.agents/skills/{self.phase}/SKILL.md'})]
        elif len(results) == 1:
            calls = [('shell', {'command': shlex.quote(sys.executable) + ' .agents/skills/documents/scripts/extract.py fixture.docx'})] if self.phase == 'documents' else [('read_file', {'path': 'source.md'})]
        elif len(results) == 2:
            text = 'Approved budget: $42. Source: fixture.docx, paragraph 1. Visual review remains open.' if self.phase == 'documents' else 'The supplied source says budget is $42. [Source](../../source.md). Evidence boundary: supplied local source only.'
            folder = 'documents' if self.phase == 'documents' else 'research'
            calls = [('write_file', {'path': f'output/{folder}/report.md', 'content': text})]
        if calls:
            delta = {'tool_calls': [{'index': i, 'id': f'work-{len(results)}-{i}', 'type': 'function', 'function': {'name': name, 'arguments': json.dumps(args)}} for i, (name, args) in enumerate(calls)]}
            finish = 'tool_calls'
        else:
            delta = {'content': 'WORK_ARTIFACT_OK'}
            finish = 'stop'
        payload = ('data: ' + json.dumps({'choices': [{'delta': delta, 'finish_reason': finish}]}) + '\n\ndata: [DONE]\n\n').encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def main():
    binary = str(Path(sys.argv[1]).resolve(strict=True))
    with tempfile.TemporaryDirectory(prefix='harness-work-smoke-') as scratch:
        root = Path(scratch) / 'real project'
        root.mkdir()
        env = {k: v for k, v in os.environ.items() if k in ('PATH', 'LANG', 'SYSTEMROOT', 'WINDIR', 'COMSPEC', 'PATHEXT')}
        env.update(HOME=scratch, USERPROFILE=scratch, HARNESS_SKIP_LSP='1')
        def run(*args):
            result = subprocess.run([binary, '-C', str(root), *args], cwd=scratch, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=45)
            assert result.returncode == 0, result.stdout + result.stderr
            return result
        for kind in ('software', 'research', 'documents', 'website', 'automation', 'apple'):
            run('work', 'Deliver a verified artifact', '--kind', kind, '--prepare-only')
            assert f'kind = "{kind}"' in (root / '.harness/build.toml').read_text()
            assert (root / f'.agents/skills/{kind}/SKILL.md').exists()
        assert not (Path(scratch) / '.harness/build.toml').exists()
        assert 'documents' in run('skills', 'list').stdout
        print('PASS six offline workflows and skill installation target selected workspace without credentials')
        office(root / 'fixture.docx', {'word/document.xml': '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Approved budget: $42</w:t></w:r></w:p></w:body></w:document>'})
        (root / 'source.md').write_text('Budget is $42 according to the supplied record.')
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
            for phase in ('documents', 'research'):
                Provider.phase = phase
                Provider.requests.clear()
                result = run('work', 'Create a sourced report', '--kind', phase)
                assert 'WORK_ARTIFACT_OK' in result.stdout
                initial = str(Provider.requests[0]['messages'][0])
                assert 'Available project skills' in initial and f'Workflow: {phase}' in initial
                assert 'scripts/extract.py beside this SKILL.md' not in initial  # Body deferred.
                outputs = [str(m) for r in Provider.requests for m in r['messages'] if m['role'] == 'tool']
                assert any('Approved budget: $42' in o for o in outputs) if phase == 'documents' else any('Budget is $42' in o for o in outputs)
                report = root / f'output/{phase}/report.md'
                assert '$42' in report.read_text()
                print(f'PASS {phase} workflow loads its skill, reads actual source, and writes artifact through tools')
            (root / '.harness/BUILD_PROGRESS.md').write_text('NEXT: check contradictory evidence')
            Provider.requests.clear()
            run('work')
            assert 'Workflow: research' in str(Provider.requests[0]['messages'][0])
            assert 'NEXT: check contradictory evidence' in str(Provider.requests[0]['messages'][0])
            print('PASS continuation retains workflow and previous evidence in fresh session')
        finally:
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    main()
