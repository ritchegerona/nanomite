#!/usr/bin/env python3
"""NANOMITE filesystem MCP server (MCP SDK v2.x).

Read-only file operations with strict path confinement to the NANOMITE
workspace root. No `..`, no absolute escapes, no symlink traversal outside
the root. All paths resolve against the workspace root.
"""

import asyncio
import os
from pathlib import Path

from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import (
    CallToolRequestParams,
    ListToolsRequest,
    PaginatedRequestParams,
    TextContent,
    Tool,
)

WORKSPACE_ROOT = Path("/Users/ritchegerona/nanomite").resolve()

DENY_DIRS = {
    "target", ".git", ".venv", "__pycache__", ".mypy_cache",
    ".pytest_cache", ".ruff_cache", "node_modules", ".opencode",
}

DENY_FILES = {".env", "config.toml", "*.sig", "*.bundle", "id_rsa",
              "id_ed25519", "tools-auth.json"}


def _is_denied(path: Path) -> bool:
    parts = path.parts
    for d in DENY_DIRS:
        if d in parts:
            return True
    name = path.name
    for pat in DENY_FILES:
        if pat.startswith("*."):
            if name.endswith(pat[1:]):
                return True
        elif pat.endswith("*"):
            if name.startswith(pat[:-1]):
                return True
        elif name == pat:
            return True
    return False


def _resolve_safe(rel: str) -> Path:
    p = (WORKSPACE_ROOT / rel).resolve()
    try:
        p.relative_to(WORKSPACE_ROOT)
    except ValueError:
        raise ValueError(f"path escapes workspace root: {rel}")
    if _is_denied(p):
        raise ValueError(f"path is denied: {rel}")
    return p


server = Server("nanomite-filesystem")


async def _list_tools_handler(ctx, params: PaginatedRequestParams):
    return {
        "tools": [
            {
                "name": "nanomite_list_files",
                "description": "List files and directories under a NANOMITE workspace path (relative to repo root).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path within the NANOMITE workspace (default: repo root).",
                            "default": "",
                        },
                    },
                },
            },
            {
                "name": "nanomite_read_file",
                "description": "Read a bounded window of a UTF-8 text file inside the NANOMITE workspace.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path to the file within the NANOMITE workspace.",
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Byte offset to start reading from.",
                            "default": 0,
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum bytes to read.",
                            "default": 65536,
                        },
                    },
                    "required": ["path"],
                },
            },
            {
                "name": "nanomite_search",
                "description": "Case-insensitive substring search over text files under a NANOMITE workspace path.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Substring to search for.",
                        },
                        "path": {
                            "type": "string",
                            "description": "Relative path to search under (default: repo root).",
                            "default": "",
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum number of matches to return.",
                            "default": 50,
                        },
                    },
                    "required": ["query"],
                },
            },
        ]
    }


async def _call_tool_handler(ctx, params: CallToolRequestParams):
    name = params.name
    arguments = params.arguments or {}

    if name == "nanomite_list_files":
        rel = arguments.get("path", "")
        p = _resolve_safe(rel)
        if not p.exists():
            return {
                "content": [{"type": "text", "text": f"not found: {rel}"}],
                "isError": True,
            }
        if p.is_file():
            return {"content": [{"type": "text", "text": p.name}]}
        entries = []
        for child in sorted(p.iterdir()):
            if _is_denied(child):
                continue
            marker = "/" if child.is_dir() else ""
            entries.append(child.name + marker)
        return {"content": [{"type": "text", "text": "\n".join(entries) if entries else "(empty)"}]}

    if name == "nanomite_read_file":
        rel = arguments["path"]
        p = _resolve_safe(rel)
        if not p.is_file():
            return {
                "content": [{"type": "text", "text": f"not a file: {rel}"}],
                "isError": True,
            }
        offset = int(arguments.get("offset", 0))
        limit = int(arguments.get("limit", 65536))
        data = p.read_bytes()[offset : offset + limit]
        text = data.decode("utf-8", errors="replace")
        return {"content": [{"type": "text", "text": text}]}

    if name == "nanomite_search":
        query = arguments["query"]
        rel = arguments.get("path", "")
        base = _resolve_safe(rel)
        max_results = int(arguments.get("max_results", 50))
        qlower = query.lower()
        results = []
        count = 0
        for root, dirs, files in os.walk(base):
            dirs[:] = [d for d in dirs if not _is_denied(Path(root) / d)]
            for fname in files:
                fpath = Path(root) / fname
                if _is_denied(fpath):
                    continue
                try:
                    data = fpath.read_bytes()
                    if b"\x00" in data[:4096]:
                        continue
                    text = data.decode("utf-8", errors="replace")
                except (OSError, UnicodeDecodeError):
                    continue
                for i, line in enumerate(text.splitlines(), 1):
                    if qlower in line.lower():
                        rel_path = fpath.relative_to(WORKSPACE_ROOT)
                        results.append(f"{rel_path}:{i}: {line[:200]}")
                        count += 1
                        if count >= max_results:
                            return {"content": [{"type": "text", "text": "\n".join(results)}]}
        return {"content": [{"type": "text", "text": "\n".join(results) if results else "(no matches)"}]}

    raise ValueError(f"unknown tool: {name}")


server.add_request_handler("tools/list", PaginatedRequestParams, _list_tools_handler)
server.add_request_handler("tools/call", CallToolRequestParams, _call_tool_handler)


async def main():
    async with stdio_server() as (read_stream, write_stream):
        await server.run(
            read_stream,
            write_stream,
            server.create_initialization_options(),
        )


if __name__ == "__main__":
    asyncio.run(main())
