#!/usr/bin/env python3
"""Wrapper/helper for Maltego CE (visual link analysis).

Maltego CE is a GUI application for link analysis and OSINT graphing. It is
not a command-line tool. This script (a) checks whether Maltego CE is
installed and launchable, and (b) exports a Maltego-compatible GraphML file
from a simple node/edge list so you can import it into Maltego CE for
visual investigation.

Requires Maltego CE installed separately:
    https://www.maltego.com/product/maltego-ce/

> Authorized use only: map infrastructure you own or have explicit written
> permission to assess. Do not use to identify or dox individuals.

Usage:
    python3 maltego.py --check
    python3 maltego.py --export nodes.csv edges.csv --out graph.graphml
"""

import argparse
import csv
import os
import shutil
import subprocess
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import List, Tuple


def default_out_path() -> Path:
    """XDG-compliant default GraphML output, independent of caller's cwd."""
    xdg = os.environ.get("XDG_DATA_HOME") or str(Path.home() / ".local" / "share")
    return Path(xdg) / "nanomite-tools" / "maltego" / "maltego_graph.graphml"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Check Maltego CE availability or export GraphML for import.",
    )
    parser.add_argument("--check", action="store_true",
                        help="Check if Maltego CE is installed/launchable.")
    parser.add_argument("--nodes", help="CSV of nodes (id,label,type).")
    parser.add_argument("--edges", help="CSV of edges (source,target,label).")
    parser.add_argument("--out", default=str(default_out_path()),
                        help="Output GraphML path "
                             "(default: XDG_DATA_HOME/nanomite-tools/maltego/maltego_graph.graphml).")
    return parser


def _is_nanomite_shim(path: str) -> bool:
    """Return True if *path* looks like a nanomite-generated shim."""
    try:
        parts = os.path.realpath(path).split(os.sep)
        return any("nanomite" in p for p in parts)
    except Exception:
        return False


def check_installed() -> int:
    """Attempt to locate the real Maltego CE binary on the host.

    Excludes nanomite's own shim directory so we don't mistake the wrapper
    for the genuine application.
    """
    candidates = ["maltego", "Maltego", "maltegoce"]
    found = []
    for c in candidates:
        path = shutil.which(c)
        if path and not _is_nanomite_shim(path):
            found.append(path)
    if found:
        print(f"[+] Maltego CE binary found: {found[0]}")
        return 0
    print("[!] Maltego CE not found on PATH. Install from "
          "https://www.maltego.com/product/maltego-ce/", file=sys.stderr)
    return 1


def read_csv(path: str) -> List[List[str]]:
    with open(path, newline="", encoding="utf-8") as fh:
        return list(csv.reader(fh))


def export_graphml(nodes_path: str, edges_path: str, out_path: str) -> int:
    nodes_rows = read_csv(nodes_path)
    edges_rows = read_csv(edges_path) if edges_path else []

    # Skip header rows if they look like headers
    def is_header(row: List[str]) -> bool:
        return any(cell.strip().lower() in ("id", "label", "type",
                                             "source", "target") for cell in row)

    nodes = [r for r in nodes_rows if r and not is_header(r)]
    edges = [r for r in edges_rows if r and not is_header(r)]

    graphml = ET.Element("graphml",
                         xmlns="http://graphml.graphdrawing.org/xmlns")
    graph = ET.SubElement(graphml, "graph", id="G", edgedefault="undirected")

    for row in nodes:
        node_id = row[0] if len(row) > 0 else ""
        label = row[1] if len(row) > 1 else node_id
        ntype = row[2] if len(row) > 2 else "unknown"
        node = ET.SubElement(graph, "node", id=node_id)
        ET.SubElement(node, "data", key="label").text = label
        ET.SubElement(node, "data", key="type").text = ntype

    for row in edges:
        src = row[0] if len(row) > 0 else ""
        dst = row[1] if len(row) > 1 else ""
        label = row[2] if len(row) > 2 else ""
        edge = ET.SubElement(graph, "edge", id=f"e{src}_{dst}",
                             source=src, target=dst)
        ET.SubElement(edge, "data", key="label").text = label

    tree = ET.ElementTree(graphml)
    ET.indent(tree, space="  ")
    tree.write(out_path, encoding="utf-8", xml_declaration=True)
    print(f"[+] GraphML written to {out_path} "
          f"({len(nodes)} nodes, {len(edges)} edges). "
          "Import into Maltego CE via File > Import > GraphML.")
    return 0


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    if args.check:
        return check_installed()

    if not args.nodes:
        parser.error("--nodes is required unless --check is given.")

    return export_graphml(args.nodes, args.edges, args.out)


if __name__ == "__main__":
    sys.exit(main())
