#!/usr/bin/env python3
"""XLSX Forensic Analysis - Extract metadata, hidden sheets, formulas, comments, data validation"""
import sys
import json
from pathlib import Path
from openpyxl import load_workbook
from openpyxl.comments import Comment

def analyze_xlsx(filepath, output_dir):
    results = {
        "file": str(filepath),
        "metadata": {},
        "sheets": [],
        "hidden_sheets": [],
        "very_hidden_sheets": [],
        "formulas": [],
        "comments": [],
        "data_validations": [],
        "hyperlinks": [],
        "defined_names": [],
        "external_links": [],
        "vba_macros": False
    }
    
    try:
        wb = load_workbook(filepath, keep_vba=True, data_only=False)
        
        # Document properties
        props = wb.properties
        results["metadata"] = {
            "title": props.title,
            "subject": props.subject,
            "creator": props.creator,
            "keywords": props.keywords,
            "category": props.category,
            "description": props.description,
            "created": str(props.created) if props.created else None,
            "modified": str(props.modified) if props.modified else None,
            "last_modified_by": props.lastModifiedBy,
            "revision": props.revision,
            "version": props.version,
            "language": props.language,
        }
        
        # Check for VBA macros
        results["vba_macros"] = wb.vba_archive is not None
        
        # Defined names (named ranges, potential hidden logic)
        try:
            for dn in wb.defined_names.definedNameList:
                results["defined_names"].append({
                    "name": dn.name,
                    "refers_to": dn.attr_text,
                    "comment": dn.comment
                })
        except AttributeError:
            # Newer openpyxl uses dict-like interface
            for name, dn in wb.defined_names.items():
                results["defined_names"].append({
                    "name": name,
                    "refers_to": dn.attr_text,
                    "comment": dn.comment
                })
        
        # External links
        if wb._external_links:
            for link in wb._external_links:
                results["external_links"].append({
                    "file": link.file_link.Target if hasattr(link, 'file_link') else str(link)
                })
        
        # Analyze each sheet
        for sheet_name in wb.sheetnames:
            ws = wb[sheet_name]
            sheet_state = ws.sheet_state  # 'visible', 'hidden', 'veryHidden'
            
            sheet_info = {
                "name": sheet_name,
                "state": sheet_state,
                "dimensions": ws.dimensions,
                "max_row": ws.max_row,
                "max_col": ws.max_column,
                "cells": [],
                "merged_cells": [str(mc) for mc in ws.merged_cells.ranges],
                "row_dimensions": dict(ws.row_dimensions),
                "col_dimensions": dict(ws.column_dimensions),
            }
            
            if sheet_state == 'hidden':
                results["hidden_sheets"].append(sheet_name)
            elif sheet_state == 'veryHidden':
                results["very_hidden_sheets"].append(sheet_name)
            
            # Scan cells for content, formulas, hyperlinks
            for row in ws.iter_rows(min_row=1, max_row=min(ws.max_row, 100), max_col=min(ws.max_column, 26), values_only=False):
                for cell in row:
                    cell_info = {
                        "coordinate": cell.coordinate,
                        "value": cell.value,
                        "data_type": cell.data_type,
                    }
                    
                    # Formulas
                    if cell.data_type == 'f':
                        cell_info["formula"] = cell.value
                        results["formulas"].append({
                            "sheet": sheet_name,
                            "cell": cell.coordinate,
                            "formula": cell.value
                        })
                    
                    # Hyperlinks
                    if cell.hyperlink:
                        cell_info["hyperlink"] = {
                            "target": cell.hyperlink.target,
                            "tooltip": cell.hyperlink.tooltip
                        }
                        results["hyperlinks"].append({
                            "sheet": sheet_name,
                            "cell": cell.coordinate,
                            "target": cell.hyperlink.target,
                            "tooltip": cell.hyperlink.tooltip
                        })
                    
                    # Comments
                    if cell.comment:
                        cell_info["comment"] = {
                            "author": cell.comment.author,
                            "text": cell.comment.text
                        }
                        results["comments"].append({
                            "sheet": sheet_name,
                            "cell": cell.coordinate,
                            "author": cell.comment.author,
                            "text": cell.comment.text
                        })
                    
                    if cell_info["value"] is not None or "formula" in cell_info or "hyperlink" in cell_info or "comment" in cell_info:
                        sheet_info["cells"].append(cell_info)
            
            # Data validations
            for dv in ws.data_validations.dataValidation:
                results["data_validations"].append({
                    "sheet": sheet_name,
                    "type": dv.type,
                    "formula1": dv.formula1,
                    "formula2": dv.formula2,
                    "allow_blank": dv.allowBlank,
                    "error": dv.error,
                    "error_title": dv.errorTitle,
                    "prompt": dv.prompt,
                    "prompt_title": dv.promptTitle,
                    "cells": [str(c) for c in dv.sqref]
                })
            
            results["sheets"].append(sheet_info)
        
    except Exception as e:
        results["error"] = str(e)
        import traceback
        results["traceback"] = traceback.format_exc()
    
    # Save detailed output
    Path(output_dir).mkdir(parents=True, exist_ok=True)
    output_path = Path(output_dir) / "xlsx_analysis.json"
    output_path.write_text(json.dumps(results, indent=2, default=str))
    
    # Print summary
    print(f"XLSX Analysis: {filepath}")
    print(f"  Metadata: {len([k for k,v in results['metadata'].items() if v])} fields")
    print(f"  Sheets: {len(results['sheets'])} total")
    print(f"  Hidden sheets: {len(results['hidden_sheets'])} - {results['hidden_sheets']}")
    print(f"  Very hidden sheets: {len(results['very_hidden_sheets'])} - {results['very_hidden_sheets']}")
    print(f"  Formulas: {len(results['formulas'])}")
    print(f"  Comments: {len(results['comments'])}")
    print(f"  Hyperlinks: {len(results['hyperlinks'])}")
    print(f"  Data validations: {len(results['data_validations'])}")
    print(f"  Defined names: {len(results['defined_names'])}")
    print(f"  External links: {len(results['external_links'])}")
    print(f"  VBA Macros: {results['vba_macros']}")
    print(f"  Results saved to: {output_path}")
    
    return results

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: xlsx_analysis.py <file> <output_dir>")
        sys.exit(1)
    analyze_xlsx(sys.argv[1], sys.argv[2])