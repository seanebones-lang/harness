#!/usr/bin/env python3
"""Release refusal and notice handling use isolated fixtures, not fake acceptance."""
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch
import subprocess
import json
import hashlib
import tarfile
import package_release
from package_release import license_html, package


class ReleasePackageTests(unittest.TestCase):
    def test_missing_or_mismatched_identity_cannot_stage_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, 'complete commit'):
                package(root, 'v1.4.0-beta.1', 'moving-main', {})
            with self.assertRaisesRegex(ValueError, 'Tag and workspace'):
                package(root, 'v0.0.0', '0' * 40, {})
            self.assertEqual(list(root.iterdir()), [])

    def test_complete_stage_is_bound_to_git_and_excludes_untracked_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'scripts').mkdir()
            (root / 'Cargo.toml').write_text('[workspace.package]\nversion = "1.4.0-beta.1"\n')
            (root / 'LICENSE').write_text('MIT license fixture')
            for name in ('install.sh', 'install.ps1'):
                (root / 'scripts' / name).write_text('fixture installer')
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            subprocess.run(['git', '-C', str(root), 'add', 'Cargo.toml', 'LICENSE', 'scripts'], check=True)
            subprocess.run(['git', '-C', str(root), '-c', 'user.name=Release fixture', '-c',
                            'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture'], check=True)
            sha = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
            (root / '.env.local').write_text('synthetic private fixture')
            artifacts = root / 'artifacts'
            artifacts.mkdir()
            report = {'licenses': [{'name': 'Fixture license', 'text': 'Fixture notice',
                                   'used_by': [{'crate': {'name': 'n11-deadbolt', 'version': '1.1.0-rc.1'}}]}]}
            with patch.object(package_release, 'ROOT', root):
                with self.assertRaisesRegex(ValueError, 'complete five'):
                    package(artifacts, 'v1.4.0-beta.1', sha, report)
                self.assertEqual(list(artifacts.iterdir()), [])
                for name in package_release.TARGETS:
                    (artifacts / name).write_bytes(b'synthetic binary fixture')
                package(artifacts, 'v1.4.0-beta.1', sha, report)
            manifest = json.loads((artifacts / 'RELEASE-MANIFEST.json').read_text())
            self.assertEqual(manifest['source_commit'], sha)
            for line in (artifacts / 'checksums.txt').read_text().splitlines():
                digest, name = line.split('  ', 1)
                self.assertEqual(hashlib.sha256((artifacts / name).read_bytes()).hexdigest(), digest)
            with tarfile.open(artifacts / 'harness-1.4.0-beta.1-source.tar.gz') as archive:
                self.assertFalse(any('.env.local' in name for name in archive.getnames()))
                self.assertTrue(any(name.endswith('/LICENSE') for name in archive.getnames()))

    def test_licenses_keep_text_and_attribution_without_html_execution(self):
        report = {'licenses': [{'name': 'Apache <test>', 'text': 'License <script> fixture',
                               'used_by': [{'crate': {'name': 'n11-deadbolt', 'version': '1.1.0-rc.1'}}]}]}
        text, count = license_html(report)
        self.assertEqual(count, 1)
        self.assertIn('n11-deadbolt 1.1.0-rc.1', text)
        self.assertIn('License &lt;script&gt; fixture', text)
        self.assertNotIn('<script>', text)
        with self.assertRaisesRegex(ValueError, 'empty'):
            license_html({'licenses': []})
        report['licenses'][0]['used_by'] = []
        with self.assertRaisesRegex(ValueError, 'DeadBolt'):
            license_html(report)


if __name__ == '__main__':
    unittest.main()
