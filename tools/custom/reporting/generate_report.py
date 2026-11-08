#!/usr/bin/env python3
"""Generate PDF and DOCX from the markdown report."""

import re
from pathlib import Path

MD_PATH = Path("/home/ritche/mailforen/report/FULL_ENUMERATION_REPORT.md")
PDF_PATH = Path("/home/ritche/mailforen/report/FULL_ENUMERATION_REPORT.pdf")
DOCX_PATH = Path("/home/ritche/mailforen/report/FULL_ENUMERATION_REPORT.docx")

CSS = """
<style>
@page {
    size: A4;
    margin: 2cm;
    @bottom-center {
        content: "Page " counter(page) " of " counter(pages);
        font-size: 9pt;
        color: #666;
    }
}
body {
    font-family: 'DejaVu Sans', sans-serif;
    font-size: 10pt;
    line-height: 1.4;
    color: #333;
}
h1 {
    font-size: 22pt;
    color: #1a1a2e;
    border-bottom: 2px solid #1a1a2e;
    padding-bottom: 0.5em;
    margin-top: 1em;
}
h2 {
    font-size: 16pt;
    color: #16213e;
    border-bottom: 1px solid #0f3460;
    padding-bottom: 0.3em;
    margin-top: 1.5em;
}
h3 {
    font-size: 13pt;
    color: #0f3460;
    margin-top: 1.2em;
}
table {
    border-collapse: collapse;
    width: 100%;
    margin: 0.5em 0;
    font-size: 9pt;
}
th {
    background-color: #0f3460;
    color: white;
    padding: 0.4em 0.6em;
    text-align: left;
    border: 1px solid #0f3460;
}
td {
    padding: 0.4em 0.6em;
    border: 1px solid #b8c4d6;
}
tr:nth-child(even) {
    background-color: #f4f6f8;
}
pre {
    background-color: #f4f4f4;
    padding: 0.8em;
    border-radius: 4px;
    font-family: 'DejaVu Sans Mono', monospace;
    font-size: 8.5pt;
    overflow-x: auto;
}
code {
    font-family: 'DejaVu Sans Mono', monospace;
    font-size: 8.5pt;
    background-color: #e8e8e8;
    padding: 0.1em 0.3em;
    border-radius: 2px;
}
ul {
    margin: 0.5em 0;
    padding-left: 1.5em;
}
li {
    margin: 0.3em 0;
}
.severity-critical { color: #c00000; font-weight: bold; }
.severity-high { color: #e67e22; font-weight: bold; }
.severity-medium { color: #f1c40f; font-weight: bold; }
.severity-low { color: #27ae60; font-weight: bold; }
hr { border: none; border-top: 1px solid #ccc; margin: 1.5em 0; }
</style>
"""


def md_to_html(md_text):
    """Convert markdown to HTML with basic syntax support."""
    html = md_text

    # Headers
    html = re.sub(r'^# (.+)$', r'<h1>\1</h1>', html, flags=re.MULTILINE)
    html = re.sub(r'^## (.+)$', r'<h2>\1</h2>', html, flags=re.MULTILINE)
    html = re.sub(r'^### (.+)$', r'<h3>\1</h3>', html, flags=re.MULTILINE)

    # Bold
    html = re.sub(r'\*\*(.+?)\*\*', r'<strong>\1</strong>', html)

    # Inline code
    html = re.sub(r'`([^`]+)`', r'<code>\1</code>', html)

    # Code blocks (fenced)
    html = re.sub(r'```(\w+)?\n(.*?)```', r'<pre><code>\2</code></pre>', html, flags=re.DOTALL)

    # Tables - simple line-based table parser
    lines = html.split('\n')
    in_table = False
    table_html = []
    result = []

    for line in lines:
        if '|' in line and not line.strip().startswith('<'):
            # Table row
            cells = [c.strip() for c in line.split('|') if c.strip()]
            if not in_table:
                in_table = True
                table_html = ['<table>']
            # Skip separator row (| --- | --- |)
            if all(set(c) <= set('-: ') for c in cells):
                continue
            tag = 'th' if len(table_html) == 1 else 'td'
            row = '<tr>' + ''.join(f'<{tag}>{cell}</{tag}>' for cell in cells) + '</tr>'
            table_html.append(row)
        else:
            if in_table:
                table_html.append('</table>')
                result.append('\n'.join(table_html))
                in_table = False
            if line.strip():
                result.append(line)

    if in_table:
        table_html.append('</table>')
        result.append('\n'.join(table_html))

    html = '\n'.join(result)

    # Paragraphs
    paragraphs = html.split('\n\n')
    html_paras = []
    for p in paragraphs:
        p = p.strip()
        if not p:
            continue
        if not p.startswith('<') and not p.startswith('---'):
            p = f'<p>{p}</p>'
        html_paras.append(p)

    html = '\n'.join(html_paras)

    # Horizontal rules
    html = html.replace('---', '<hr>')

    # Wrap in body
    html = f'<body>{CSS}\n{html}\n</body>'

    return html


def convert_to_pdf(html_path, pdf_path):
    """Convert HTML to PDF using weasyprint."""
    from weasyprint import HTML
    HTML(str(html_path)).write_pdf(str(pdf_path))
    print(f"[+] PDF saved: {pdf_path}")


def convert_to_docx(md_path, docx_path):
    """Convert markdown to DOCX using python-docx."""
    from docx import Document
    from docx.shared import Inches, Pt, RGBColor

    doc = Document()

    # Set base style
    style = doc.styles['Normal']
    style.font.name = 'Calibri'
    style.font.size = Pt(11)

    with open(md_path, 'r') as f:
        lines = f.readlines()

    i = 0
    table_data = []

    while i < len(lines):
        line = lines[i].rstrip('\n')

        # Skip empty lines between blocks
        if not line.strip():
            i += 1
            continue

        # Header 1
        if line.startswith('# '):
            doc.add_heading(line[2:].strip(), level=0)
            i += 1
            continue

        # Header 2
        if line.startswith('## '):
            doc.add_heading(line[3:].strip(), level=1)
            i += 1
            continue

        # Header 3
        if line.startswith('### '):
            doc.add_heading(line[4:].strip(), level=2)
            i += 1
            continue

        # Horizontal rule
        if line.strip() == '---':
            doc.add_paragraph('_' * 80)
            i += 1
            continue

        # Table
        if '|' in line:
            table_data = []
            # Collect table lines
            while i < len(lines) and '|' in lines[i]:
                tline = lines[i].strip()
                if tline and not all(set(c) <= set('-: |') for c in tline.replace('|', '')):
                    cells = [c.strip() for c in tline.split('|') if c.strip()]
                    table_data.append(cells)
                i += 1
            if table_data:
                table = doc.add_table(rows=1, cols=len(table_data[0]))
                table.style = 'Table Grid'
                # Header row
                hdr = table.rows[0].cells
                for j, cell_text in enumerate(table_data[0]):
                    hdr[j].text = cell_text
                    for paragraph in hdr[j].paragraphs:
                        for run in paragraph.runs:
                            run.font.bold = True
                            run.font.color.rgb = RGBColor(0xFF, 0xFF, 0xFF)
                    # Shade header
                    from docx.oxml import OxmlElement
                    from docx.oxml.ns import qn
                    tcPr = hdr[j]._tc.get_or_add_tcPr()
                    shd = OxmlElement('w:shd')
                    shd.set(qn('w:val'), 'clear')
                    shd.set(qn('w:fill'), '0F3460')
                    tcPr.append(shd)
                # Data rows
                for row_data in table_data[1:]:
                    row = table.add_row().cells
                    for j, cell_text in enumerate(row_data):
                        row[j].text = cell_text
            continue

        # Code block
        if line.startswith('```'):
            i += 1
            code_lines = []
            while i < len(lines) and not lines[i].startswith('```'):
                code_lines.append(lines[i].rstrip('\n'))
                i += 1
            i += 1  # skip closing ```
            code_text = '\n'.join(code_lines)
            p = doc.add_paragraph()
            run = p.add_run(code_text)
            run.font.name = 'Consolas'
            run.font.size = Pt(9)
            p.paragraph_format.left_indent = Inches(0.25)
            continue

        # Bullet list
        if line.strip().startswith('- ') or line.strip().startswith('* '):
            doc.add_paragraph(line.strip()[2:], style='List Bullet')
            i += 1
            continue

        # Regular paragraph - handle bold
        p = doc.add_paragraph()
        # Simple bold parsing: **text**
        parts = re.split(r'(\*\*.+?\*\*)', line)
        for part in parts:
            if part.startswith('**') and part.endswith('**'):
                run = p.add_run(part[2:-2])
                run.bold = True
            elif part:
                p.add_run(part)
        i += 1

    doc.save(str(docx_path))
    print(f"[+] DOCX saved: {docx_path}")


def main():
    md_text = MD_PATH.read_text()

    # Generate HTML for PDF
    html = md_to_html(md_text)
    html_path = Path("/home/ritche/mailforen/report/_report.html")
    html_path.write_text(html)

    # Generate PDF
    convert_to_pdf(html_path, PDF_PATH)

    # Generate DOCX
    convert_to_docx(MD_PATH, DOCX_PATH)

    # Cleanup temp HTML
    html_path.unlink()

    print("\n[+] All reports generated in /home/ritche/mailforen/report/")
    print("    - FULL_ENUMERATION_REPORT.md")
    print("    - FULL_ENUMERATION_REPORT.pdf")
    print("    - FULL_ENUMERATION_REPORT.docx")


if __name__ == '__main__':
    main()