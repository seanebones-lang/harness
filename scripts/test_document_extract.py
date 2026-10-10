#!/usr/bin/env python3
"""Independent Office fixtures verify extraction, source locations, and formula honesty."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

SCRIPT = Path(__file__).resolve().parents[1] / 'assets/workflows/documents/scripts/extract.py'
spec = importlib.util.spec_from_file_location('extract', SCRIPT)
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


def office(path, parts):
    with zipfile.ZipFile(path, 'w') as archive:
        for name, text in parts.items():
            archive.writestr(name, text)


class ExtractionTests(unittest.TestCase):
    def test_pdf_trailing_page_break_does_not_invent_a_source_page(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / 'single-page.pdf'
            path.write_bytes(b'%PDF fixture')
            def extractor(args, **_):
                Path(args[-1]).write_text('Approved budget: $42\f')
            with patch.object(helper.shutil, 'which', return_value='pdftotext'), patch.object(helper.subprocess, 'run', side_effect=extractor):
                result = helper.extract(path)
            self.assertEqual(result['segments'], [{'location': 'page:1', 'text': 'Approved budget: $42'}])

    def test_office_sources_and_cached_formulas(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            office(root / 'fixture.docx', {'word/document.xml': '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Approved </w:t></w:r><w:r><w:t>budget: $42</w:t></w:r></w:p></w:body></w:document>'})
            word = helper.extract(root / 'fixture.docx')
            self.assertEqual(word['segments'], [{'location': 'paragraph:1', 'text': 'Approved budget: $42'}])
            office(root / 'fixture.pptx', {'ppt/slides/slide10.xml': '<a:p xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:t>Last slide</a:t></a:p>', 'ppt/slides/slide2.xml': '<a:p xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:t>Earlier slide</a:t></a:p>'})
            slides = helper.extract(root / 'fixture.pptx')['segments']
            self.assertEqual([s['text'] for s in slides], ['Earlier slide', 'Last slide'])
            office(root / 'fixture.xlsx', {
                'xl/workbook.xml': '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Budget" r:id="rId7"/></sheets></workbook>',
                'xl/_rels/workbook.xml.rels': '<Relationships><Relationship Id="rId7" Target="worksheets/sheet3.xml"/></Relationships>',
                'xl/sharedStrings.xml': '<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>Cost</t></si></sst>',
                'xl/worksheets/sheet3.xml': '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row><c r="A1" t="s"><v>0</v></c><c r="B1"><f>6*7</f><v>99</v></c><c r="C1"><f>7*8</f></c></row></sheetData></worksheet>'})
            cells = helper.extract(root / 'fixture.xlsx')['segments']
            self.assertEqual(cells[0], {'location': 'Budget!A1', 'text': 'Cost'})
            self.assertEqual(cells[1]['text'], '99')  # Cached result intentionally differs from formula.
            self.assertEqual(cells[1]['formula'], '6*7')
            self.assertEqual(cells[1]['value_status'], 'cached_not_recalculated')
            self.assertEqual(cells[2]['text'], '')

    def test_limits_and_unsupported_inputs_fail_explicitly(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / 'rows.csv'
            path.write_text('a,b\n' * 2_000)
            result = helper.extract(path)
            self.assertTrue(result['truncated'])
            self.assertEqual(len(result['segments']), 1_000)
            path = Path(td) / 'danger.docx'
            office(path, {'word/document.xml': '<!DOCTYPE d [<!ENTITY a "x">]><d>&a;</d>'})
            with self.assertRaisesRegex(ValueError, 'entities'):
                helper.extract(path)
            path = Path(td) / 'unknown.bin'
            path.write_bytes(b'\xff')
            result = subprocess.run([sys.executable, str(SCRIPT), str(path)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn('unsupported format', result.stderr)
            path = Path(td) / 'fixture.pdf'
            path.write_bytes(b'%PDF-1.4\n')
            result = subprocess.run([sys.executable, str(SCRIPT), str(path)], env={'PATH': ''}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn('requires pdftotext', result.stderr)


if __name__ == '__main__':
    unittest.main()
