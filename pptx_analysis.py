#!/usr/bin/env python3
"""PPTX Forensic Analysis - Extract metadata, hidden slides, notes, comments, embedded objects"""
import sys
import json
from pathlib import Path
from pptx import Presentation
from pptx.opc.constants import RELATIONSHIP_TYPE as RT

def analyze_pptx(filepath, output_dir):
    results = {
        "file": str(filepath),
        "metadata": {},
        "slides": [],
        "hidden_slides": [],
        "notes": [],
        "comments": [],
        "embedded_objects": [],
        "hyperlinks": [],
        "master_slides": [],
        "layouts": []
    }
    
    try:
        prs = Presentation(filepath)
        
        # Core properties
        props = prs.core_properties
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
        
        # Slide master and layouts
        for master in prs.slide_masters:
            master_info = {
                "name": master.name,
                "layouts": []
            }
            for layout in master.slide_layouts:
                master_info["layouts"].append({
                    "name": layout.name,
                    "placeholders": [ph.placeholder_format.type for ph in layout.placeholders]
                })
            results["master_slides"].append(master_info)
        
        # Analyze slides
        for i, slide in enumerate(prs.slides):
            # Check for hidden attribute - it's stored in the presentation's sldIdLst as 'show' attribute
            # or on the slide element itself
            ns = '{http://schemas.openxmlformats.org/presentationml/2006/main}'
            is_hidden = slide.part._element.attrib.get(ns + 'show') == '0'
            
            # Also check the presentation's sldIdLst for show attribute
            sld_id_lst = prs.part._element.find('p:sldIdLst', {'p': 'http://schemas.openxmlformats.org/presentationml/2006/main'})
            if sld_id_lst is not None and i < len(sld_id_lst):
                sld_id = sld_id_lst[i]
                if sld_id.get(ns + 'show') == '0':
                    is_hidden = True
            
            slide_info = {
                "index": i,
                "layout": slide.slide_layout.name if slide.slide_layout else None,
                "hidden": is_hidden,
                "shapes": [],
                "notes": None
            }
            
            if is_hidden:
                results["hidden_slides"].append(i)
            
            # Shapes
            for shape in slide.shapes:
                shape_info = {
                    "name": shape.name,
                    "shape_type": str(shape.shape_type),
                    "left": shape.left,
                    "top": shape.top,
                    "width": shape.width,
                    "height": shape.height,
                    "text": None
                }
                
                # Text content
                if shape.has_text_frame:
                    text_parts = []
                    for para in shape.text_frame.paragraphs:
                        para_text = para.text.strip()
                        if para_text:
                            text_parts.append(para_text)
                        # Check hyperlinks on runs
                        for run in para.runs:
                            if run.hyperlink and run.hyperlink.address:
                                results["hyperlinks"].append({
                                    "slide": i,
                                    "shape": shape.name,
                                    "paragraph": para.text[:50],
                                    "run": run.text[:50],
                                    "target": run.hyperlink.address,
                                    "sub_address": getattr(run.hyperlink, 'sub_address', None)
                                })
                    if text_parts:
                        shape_info["text"] = "\n".join(text_parts)[:500]
                
                # Hyperlinks (click action)
                if shape.click_action and shape.click_action.hyperlink:
                    hl = shape.click_action.hyperlink
                    shape_info["hyperlink"] = {
                        "target": hl.address,
                        "sub_address": getattr(hl, 'sub_address', None)
                    }
                    results["hyperlinks"].append({
                        "slide": i,
                        "shape": shape.name,
                        "target": hl.address,
                        "sub_address": getattr(hl, 'sub_address', None)
                    })
                
                # Embedded objects (OLE)
                if shape.shape_type == 7:  # MSO_SHAPE_TYPE.EMBEDDED_OLE_OBJECT
                    shape_info["ole_object"] = True
                    results["embedded_objects"].append({
                        "slide": i,
                        "shape": shape.name,
                        "prog_id": getattr(shape, '_element', {}).get('progId', 'unknown')
                    })
                
                # Tables
                if shape.has_table:
                    table_data = []
                    for row in shape.table.rows:
                        row_data = [cell.text.strip() for cell in row.cells]
                        table_data.append(row_data)
                    shape_info["table"] = {
                        "rows": len(shape.table.rows),
                        "cols": len(shape.table.columns),
                        "data": table_data[:20]
                    }
                
                # Charts
                if shape.has_chart:
                    shape_info["chart"] = True
                
                slide_info["shapes"].append(shape_info)
            
            # Notes
            if slide.has_notes_slide:
                notes_text = ""
                for shape in slide.notes_slide.shapes:
                    if shape.has_text_frame:
                        for para in shape.text_frame.paragraphs:
                            notes_text += para.text + "\n"
                if notes_text.strip():
                    slide_info["notes"] = notes_text.strip()[:500]
                    results["notes"].append({
                        "slide": i,
                        "text": notes_text.strip()[:500]
                    })
            
            results["slides"].append(slide_info)
        
        # Comments (if available)
        try:
            for slide in prs.slides:
                if hasattr(slide, '_comments') and slide._comments:
                    for comment in slide._comments:
                        results["comments"].append({
                            "slide": slide.slide_id,
                            "author": comment.author,
                            "text": comment.text,
                            "date": str(comment.date) if comment.date else None
                        })
        except:
            pass
        
        # Check relationships for embedded files
        for rel in prs.part.rels.values():
            if 'oleObject' in rel.reltype or 'package' in rel.reltype:
                results["embedded_objects"].append({
                    "target": rel.target_ref,
                    "reltype": rel.reltype
                })
        
    except Exception as e:
        results["error"] = str(e)
        import traceback
        results["traceback"] = traceback.format_exc()
    
    # Save detailed output
    Path(output_dir).mkdir(parents=True, exist_ok=True)
    output_path = Path(output_dir) / "pptx_analysis.json"
    output_path.write_text(json.dumps(results, indent=2, default=str))
    
    # Print summary
    print(f"PPTX Analysis: {filepath}")
    print(f"  Metadata: {len([k for k,v in results['metadata'].items() if v])} fields")
    print(f"  Slides: {len(results['slides'])}")
    print(f"  Hidden slides: {len(results['hidden_slides'])} - {results['hidden_slides']}")
    print(f"  Notes slides: {len(results['notes'])}")
    print(f"  Comments: {len(results['comments'])}")
    print(f"  Embedded objects: {len(results['embedded_objects'])}")
    print(f"  Hyperlinks: {len(results['hyperlinks'])}")
    print(f"  Master slides: {len(results['master_slides'])}")
    print(f"  Results saved to: {output_path}")
    
    return results

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: pptx_analysis.py <file> <output_dir>")
        sys.exit(1)
    analyze_pptx(sys.argv[1], sys.argv[2])