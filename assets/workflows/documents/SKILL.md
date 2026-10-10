---
name: documents
description: Read, create, or revise PDFs, Word documents, presentations, spreadsheets, and written deliverables.
---

# Documents workflow

Identify the requested output format and inspect the input before rewriting it. Use scripts/extract.py beside this SKILL.md for bounded text extraction from DOCX, XLSX, PPTX, PDF, CSV, Markdown, and text. It uses Python standard libraries for Office files; PDF extraction requires pdftotext. Extracted text does not prove layout fidelity, formula evaluation, image interpretation, OCR, or tracked-change preservation.
For formatted creation or editing, check the actual dependencies first: python-docx for Word, openpyxl for workbooks, python-pptx for slides, reportlab for PDF, LibreOffice or Poppler for rendering. Use a project virtual environment if installation is needed and authorized; retain the source files. Review spreadsheet formulas and units and verify computed values independently where material. Render final pages or slides and inspect screenshots through an available image-capable tool/model, or explicitly leave visual review open. Do not claim visual inspection from extracted text. Save final files under output/documents/ and provide exact filenames and any open gates. Preserve the user's meaning and verified claims; do not fabricate credentials, customers, or results.
