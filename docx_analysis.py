#!/usr/bin/env python3
"""DOCX Forensic Analysis - Extract metadata, hidden content, comments, headers/footers"""
import sys
import json
from pathlib import Path
from docx import Document
from docx.opc.coreprops import CoreProperties

def analyze_docx(filepath, output_dir):
    results = {
        "file": str(filepath),
        "metadata": {},
        "content": [],
        "hidden_content": [],
        "headers_footers": {},
        "comments": [],
        "tables": [],
        "structure": {}
    }
    
    try:
        doc = Document(filepath)
        
        # Core properties
        props = doc.core_properties
        results["metadata"] = {
            "title": props.title,
            "author": props.author,
            "subject": props.subject,
            "keywords": props.keywords,
            "category": props.category,
            "comments": props.comments,
            "created": str(props.created) if props.created else None,
            "modified": str(props.modified) if props.modified else None,
            "last_modified_by": props.last_modified_by,
            "revision": props.revision,
            "version": props.version,
            "language": props.language,
        }
        
        # Paragraphs with style info
        for i, para in enumerate(doc.paragraphs):
            text = para.text.strip()
            if text:
                results["content"].append({
                    "index": i,
                    "style": para.style.name,
                    "text": text[:500],  # Limit length
                    "runs": len(para.runs)
                })
            # Check for hidden text (white font, very small, etc.)
            for run in para.runs:
                if run.font.color and run.font.color.rgb:
                    if str(run.font.color.rgb).upper() in ['FFFFFF', '000000']:
                        results["hidden_content"].append({
                            "type": "font_color",
                            "paragraph": i,
                            "text": run.text[:200],
                            "color": str(run.font.color.rgb)
                        })
                if run.font.size and run.font.size.pt and run.font.size.pt < 2:
                    results["hidden_content"].append({
                        "type": "tiny_font",
                        "paragraph": i,
                        "text": run.text[:200],
                        "size_pt": run.font.size.pt
                    })
        
        # Headers and Footers
        for section in doc.sections:
            section_results = {"header": [], "footer": []}
            for para in section.header.paragraphs:
                if para.text.strip():
                    section_results["header"].append(para.text[:500])
            for para in section.footer.paragraphs:
                if para.text.strip():
                    section_results["footer"].append(para.text[:500])
            results["headers_footers"][f"section_{len(results['headers_footers'])}"] = section_results
        
        # Tables
        for t_idx, table in enumerate(doc.tables):
            table_data = []
            for row in table.rows:
                row_data = [cell.text.strip() for cell in row.cells]
                table_data.append(row_data)
            results["tables"].append({
                "index": t_idx,
                "rows": len(table.rows),
                "cols": len(table.columns),
                "data": table_data[:20]  # Limit rows
            })
        
        # Comments (if available in newer python-docx)
        try:
            from docx.opc.comments import Comments
            if doc.part.package.part_related_by(doc.opc.constants.RELATIONSHIP_TYPE.COMMENTS):
                comments_part = doc.part.package.part_related_by(doc.opc.constants.RELATIONSHIP_TYPE.COMMENTS)
                for comment in comments_part.comments:
                    results["comments"].append({
                        "author": comment.author,
                        "text": comment.text,
                        "date": str(comment.date) if comment.date else None
                    })
        except:
            pass
        
        # Document structure
        results["structure"] = {
            "paragraphs": len(doc.paragraphs),
            "tables": len(doc.tables),
            "sections": len(doc.sections),
            "styles": [s.name for s in doc.styles]
        }
        
        # Check for embedded objects
        for rel in doc.part.rels.values():
            if 'oleObject' in rel.reltype or 'package' in rel.reltype:
                results["hidden_content"].append({
                    "type": "embedded_object",
                    "target": rel.target_ref,
                    "reltype": rel.reltype
                })
        
    except Exception as e:
        results["error"] = str(e)
    
    # Save detailed output
    Path(output_dir).mkdir(parents=True, exist_ok=True)
    output_path = Path(output_dir) / "docx_analysis.json"
    output_path.write_text(json.dumps(results, indent=2))
    
    # Print summary
    print(f"DOCX Analysis: {filepath}")
    print(f"  Metadata: {len([k for k,v in results['metadata'].items() if v])} fields")
    print(f"  Paragraphs: {len(results['content'])}")
    print(f"  Hidden content indicators: {len(results['hidden_content'])}")
    print(f"  Headers/Footers: {len(results['headers_footers'])} sections")
    print(f"  Tables: {len(results['tables'])}")
    print(f"  Comments: {len(results['comments'])}")
    print(f"  Results saved to: {output_path}")
    
    return results

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: docx_analysis.py <file> <output_dir>")
        sys.exit(1)
    analyze_docx(sys.argv[1], sys.argv[2])