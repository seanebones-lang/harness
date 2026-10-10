#!/usr/bin/env python3
"""Extract bounded source text; does not render, OCR, or evaluate Office formulas."""
import argparse
import csv
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile

MAX_INPUT = 32 * 1024 * 1024
MAX_TEXT = 200_000
MAX_SEGMENTS = 1_000
W = '{http://schemas.openxmlformats.org/wordprocessingml/2006/main}'
S = '{http://schemas.openxmlformats.org/spreadsheetml/2006/main}'
A = '{http://schemas.openxmlformats.org/drawingml/2006/main}'
R = '{http://schemas.openxmlformats.org/officeDocument/2006/relationships}'


def extract(path):
    path = Path(path)
    if path.stat().st_size > MAX_INPUT:
        raise ValueError('input exceeds 32 MiB')
    kind = path.suffix.lower().lstrip('.')
    result = {'source': str(path), 'format': kind, 'segments': [], 'truncated': False,
              'limitations': 'Text extraction only; layout, OCR, images, and formula evaluation are unverified.'}
    remaining = MAX_TEXT

    def add(location, text, **extra):
        nonlocal remaining
        if len(result['segments']) >= MAX_SEGMENTS or remaining <= 0:
            result['truncated'] = True
            return False
        clipped = text[:min(remaining, 4_000)]
        result['truncated'] |= len(clipped) < len(text)
        remaining -= len(clipped)
        result['segments'].append({'location': location, 'text': clipped, **extra})
        return True

    if kind in ('docx', 'xlsx', 'pptx'):
        with zipfile.ZipFile(path) as archive:
            entries = archive.infolist()
            if len(entries) > 2_000 or sum(e.file_size for e in entries) > MAX_INPUT:
                raise ValueError('expanded Office archive exceeds 32 MiB or 2000 entries')

            def xml(name):
                data = archive.read(name)
                if b'<!DOCTYPE' in data.upper() or b'<!ENTITY' in data.upper():
                    raise ValueError('XML entities and DTDs are unsupported')
                return ET.fromstring(data)

            if kind == 'docx':
                for i, paragraph in enumerate(xml('word/document.xml').iter(W + 'p'), 1):
                    text = ''.join(e.text or '' for e in paragraph.iter(W + 't'))
                    if text and not add(f'paragraph:{i}', text):
                        break
            elif kind == 'pptx':
                slides = [n for n in archive.namelist() if n.startswith('ppt/slides/slide') and n[16:-4].isdigit() and n.endswith('.xml')]
                for name in sorted(slides, key=lambda n: int(n[16:-4])):
                    text = '\n'.join(e.text or '' for e in xml(name).iter(A + 't'))
                    if not add(name, text):
                        break
            else:
                strings = []
                if 'xl/sharedStrings.xml' in archive.namelist():
                    strings = [''.join(e.text or '' for e in si.iter(S + 't')) for si in xml('xl/sharedStrings.xml').iter(S + 'si')]
                relationships = {e.attrib['Id']: e.attrib['Target'] for e in xml('xl/_rels/workbook.xml.rels')}
                for sheet in xml('xl/workbook.xml').iter(S + 'sheet'):
                    target = relationships[sheet.attrib[R + 'id']]
                    name = target.lstrip('/') if target.startswith('/') else 'xl/' + target
                    if '..' in Path(name).parts or not name.startswith('xl/'):
                        raise ValueError('unsupported worksheet relationship')
                    for cell in xml(name).iter(S + 'c'):
                        value = cell.find(S + 'v')
                        text = value.text or '' if value is not None else ''
                        if cell.attrib.get('t') == 's':
                            text = strings[int(text)]
                        elif cell.attrib.get('t') == 'inlineStr':
                            text = ''.join(e.text or '' for e in cell.iter(S + 't'))
                        formula = cell.find(S + 'f')
                        extra = {'formula': formula.text or '', 'value_status': 'cached_not_recalculated'} if formula is not None else {}
                        if not add(sheet.attrib['name'] + '!' + cell.attrib['r'], text, **extra):
                            break
                    if len(result['segments']) >= MAX_SEGMENTS or remaining <= 0:
                        result['truncated'] = True
                        break
    elif kind == 'pdf':
        executable = shutil.which('pdftotext')
        if not executable:
            raise ValueError('PDF extraction requires pdftotext (Poppler); on macOS: brew install poppler')
        with tempfile.TemporaryDirectory(prefix='harness-pdf-') as scratch:
            output = Path(scratch) / 'text.txt'
            subprocess.run([executable, '-layout', str(path.resolve()), str(output)], check=True, timeout=30, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            with output.open(encoding='utf-8') as stream:
                text = stream.read(MAX_TEXT + 1)
            result['truncated'] |= len(text) > MAX_TEXT
            for i, page in enumerate(text[:MAX_TEXT].split('\f'), 1):
                if not add(f'page:{i}', page):
                    break
    elif kind in ('csv', 'tsv'):
        with path.open(encoding='utf-8-sig', newline='') as stream:
            for i, row in enumerate(csv.reader(stream, delimiter='\t' if kind == 'tsv' else ','), 1):
                if not add(f'row:{i}', json.dumps(row, ensure_ascii=False)):
                    break
    elif kind in ('txt', 'md', 'json', 'html', 'xml', 'log'):
        with path.open(encoding='utf-8-sig') as stream:
            text = stream.read(MAX_TEXT + 1)
        result['truncated'] |= len(text) > MAX_TEXT
        for i in range(0, len(text[:MAX_TEXT]), 4_000):
            if not add(f'chars:{i + 1}', text[i:min(i + 4_000, MAX_TEXT)]):
                break
    else:
        raise ValueError(f'unsupported format: {kind}; use the appropriate reader or supply text')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('path', nargs='?')
    parser.add_argument('--probe', action='store_true', help='Report optional extraction/creation dependencies without installing them')
    args = parser.parse_args()
    if args.probe:
        print(json.dumps({'python': sys.version.split()[0], 'tools': {n: shutil.which(n) is not None for n in ('pdftotext', 'pdftoppm', 'libreoffice')},
                          'packages': {n: importlib.util.find_spec(n) is not None for n in ('docx', 'openpyxl', 'pptx', 'reportlab')}}, indent=2))
    elif not args.path:
        parser.error('provide a file path or --probe')
    else:
        try:
            print(json.dumps(extract(args.path), ensure_ascii=False, indent=2))
        except (OSError, ValueError, KeyError, IndexError, ET.ParseError, zipfile.BadZipFile, subprocess.SubprocessError) as error:
            print(f'extraction failed: {error}', file=sys.stderr)
            sys.exit(2)


if __name__ == '__main__':
    main()
