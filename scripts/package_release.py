#!/usr/bin/env python3
"""Package an exact reviewed commit and complete native matrix, never local state."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import re
import shutil
import subprocess

TARGETS = {
    'harness-macos-aarch64': 'aarch64-apple-darwin',
    'harness-macos-x86_64': 'x86_64-apple-darwin',
    'harness-linux-aarch64': 'aarch64-unknown-linux-gnu',
    'harness-linux-x86_64': 'x86_64-unknown-linux-gnu',
    'harness-windows-x86_64.exe': 'x86_64-pc-windows-msvc',
}
ROOT = Path(__file__).resolve().parent.parent


def checksum(data):
    return hashlib.sha256(data).hexdigest()


def license_html(report):
    licenses = report['licenses']
    if not licenses:
        raise ValueError('Dependency license report is empty')
    crates = set()
    sections = []
    for license in licenses:
        text = license['text']
        if not text.strip():
            raise ValueError('Dependency license text is missing')
        used = []
        for entry in license['used_by']:
            crate = entry['crate']
            crates.add((crate['name'], crate['version']))
            used.append(html.escape(crate['name'] + ' ' + crate['version']))
        sections.append('<section><h2>' + html.escape(license['name']) +
                        '</h2><p>' + ', '.join(used) + '</p><pre>' +
                        html.escape(text) + '</pre></section>')
    if not any(name == 'n11-deadbolt' for name, _ in crates):
        raise ValueError('Default DeadBolt dependency missing from report')
    heading = ('<!doctype html><html lang="en"><meta charset="utf-8">'
               '<title>Harness dependency licenses</title><style>'
               'body{font:16px/1.5 system-ui;max-width:1000px;margin:3rem auto;padding:0 1rem}'
               'pre{white-space:pre-wrap;overflow-wrap:anywhere}section{border-top:1px solid #aaa}'
               '</style><h1>Harness dependency licenses</h1><p>'
               'Generated with cargo-about 0.9.2 from the locked default CLI graph across '
               'the five release targets. Build/dev-only dependencies are excluded. '
               'This report retains discovered license text and attribution; it is not '
               'a legal certification or an inventory of optional desktop/editor clients. '
               'Harness itself is covered by the accompanying LICENSE.txt.</p>')
    return (heading + ''.join(sections) + '</html>'), len(crates)


def package(directory, tag, sha, report):
    if not re.fullmatch(r'[0-9a-f]{40}', sha):
        raise ValueError('A complete commit identity is required')
    section = re.search(r'(?ms)^\[workspace\.package\]\s*\n((?:(?!^\[).)*)', (ROOT / 'Cargo.toml').read_text())
    if not section:
        raise ValueError('Workspace package metadata missing')
    match = re.search(r'^version\s*=\s*"([^"\n]+)"', section[1], re.MULTILINE)
    if not match:
        raise ValueError('Workspace version missing')
    version = match[1]
    if tag != 'v' + version:
        raise ValueError('Tag and workspace version differ')
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if head != sha:
        raise ValueError('Checkout differs from requested source commit')
    if subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=ROOT):
        raise ValueError('Commit tracked changes before packaging')
    if set(p.name for p in directory.iterdir()) != set(TARGETS):
        raise ValueError('Expected exactly the complete five native binaries')
    for name in TARGETS:
        if not (directory / name).is_file() or (directory / name).stat().st_size == 0:
            raise ValueError('Missing or empty native artifact: ' + name)
    license_text, crate_count = license_html(report)
    (directory / 'THIRD-PARTY-LICENSES.html').write_text(license_text, encoding='utf-8')
    shutil.copyfile(ROOT / 'LICENSE', directory / 'LICENSE.txt')
    for name in ('install.sh', 'install.ps1'):
        shutil.copyfile(ROOT / 'scripts' / name, directory / name)
    source = 'harness-' + version + '-source.tar.gz'
    subprocess.run(['git', 'archive', '--format=tar.gz', '--prefix=harness-' + version + '/',
                    '--output=' + str((directory / source).resolve()), sha], cwd=ROOT, check=True)
    assets = []
    for path in sorted(directory.iterdir()):
        data = path.read_bytes()
        item = {'name': path.name, 'bytes': len(data), 'sha256': checksum(data)}
        if path.name in TARGETS:
            item['target'] = TARGETS[path.name]
        assets.append(item)
    manifest = {
        'version': version, 'tag': tag, 'source_commit': sha,
        'profile': 'release-lto', 'default_features': ['deadbolt'],
        'harness_license': 'MIT', 'dependency_license_tool': 'cargo-about 0.9.2',
        'dependency_license_crates': crate_count,
        'release_scope': 'developer prerelease' if '-' in version else 'release candidate',
        'manual_all_platform_acceptance': False, 'assets': assets,
    }
    manifest_bytes = (json.dumps(manifest, indent=2) + '\n').encode()
    (directory / 'RELEASE-MANIFEST.json').write_bytes(manifest_bytes)
    assets.append({'name': 'RELEASE-MANIFEST.json', 'sha256': checksum(manifest_bytes)})
    (directory / 'checksums.txt').write_text(''.join(a['sha256'] + '  ' + a['name'] + '\n' for a in assets))
    print(f'Packaged {tag} at {sha}: five binaries, source, installers, license report, manifest and checksums')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts', required=True, type=Path)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--sha', required=True)
    parser.add_argument('--licenses', required=True, type=Path)
    args = parser.parse_args()
    package(args.artifacts, args.tag, args.sha, json.loads(args.licenses.read_text()))
