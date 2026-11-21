#!/usr/bin/env python3
"""NANOMITE tool catalog MCP server (MCP SDK v2.x).

Exposes NANOMITE's tool inventory (from src/tools/catalog.rs) as MCP tools
and resources. Read-only: parses the Rust source, never modifies it.
"""

import asyncio
import re
from pathlib import Path

from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import (
    CallToolRequestParams,
    ListToolsRequest,
    PaginatedRequestParams,
    ReadResourceRequestParams,
)

CATALOG_PATH = Path("/Users/ritchegerona/nanomite/src/tools/catalog.rs")

_TOOL_RE = re.compile(
    r'name:\s*"([^"]+)",\s*description:\s*"([^"]+)",\s*category:\s*(\w+)',
    re.MULTILINE | re.DOTALL,
)

_CATEGORY_MAP = {
    "Files": "files",
    "Exec": "exec",
    "Web": "web",
    "Memory": "memory",
    "Document": "document",
    "SelfImprove": "self_improve",
    "Git": "git",
    "Test": "test",
    "Index": "index",
    "Plan": "plan",
}


def _load_catalog():
    text = CATALOG_PATH.read_text()
    tools = []
    for m in _TOOL_RE.finditer(text):
        name = m.group(1)
        description = m.group(2)
        category = _CATEGORY_MAP.get(m.group(3), "other")
        tools.append(
            {
                "name": name,
                "description": description,
                "category": category,
            }
        )
    return tools


TOOL_DEFS = [
    {
        "name": "nanomite_catalog_list",
        "description": "List all tools in NANOMITE's agent tool catalog (name, description, category).",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "nanomite_catalog_get",
        "description": "Get the spec for one NANOMITE tool by name.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Tool name (e.g. run_command, memory_search, cargo_test_lib).",
                },
            },
            "required": ["name"],
        },
    },
]

server = Server("nanomite-catalog")


async def _list_tools_handler(ctx, params: PaginatedRequestParams):
    return {"tools": TOOL_DEFS}


async def _call_tool_handler(ctx, params: CallToolRequestParams):
    name = params.name
    arguments = params.arguments or {}
    tools = _load_catalog()

    if name == "nanomite_catalog_list":
        lines = [
            f"{t['name']} [{t['category']}]: {t['description']}" for t in tools
        ]
        return {"content": [{"type": "text", "text": "\n".join(lines)}]}

    if name == "nanomite_catalog_get":
        wanted = arguments["name"]
        for t in tools:
            if t["name"] == wanted:
                lines = [
                    f"name: {t['name']}",
                    f"category: {t['category']}",
                    f"description: {t['description']}",
                ]
                return {"content": [{"type": "text", "text": "\n".join(lines)}]}
        return {
            "content": [{"type": "text", "text": f"tool not found: {wanted}"}],
            "isError": True,
        }

    raise ValueError(f"unknown tool: {name}")


async def _list_resources_handler(ctx, params: PaginatedRequestParams):
    tools = _load_catalog()
    return {
        "resources": [
            {
                "uri": f"nanomite://catalog/{t['name']}",
                "name": t["name"],
                "description": t["description"],
                "mimeType": "text/plain",
            }
            for t in tools
        ]
    }


async def _read_resource_handler(ctx, params: ReadResourceRequestParams):
    uri = params.uri
    tools = _load_catalog()
    name = uri.removeprefix("nanomite://catalog/")
    for t in tools:
        if t["name"] == name:
            return {
                "contents": [
                    {
                        "type": "text",
                        "text": f"{t['name']} [{t['category']}]\n{t['description']}",
                    }
                ]
            }
    raise ValueError(f"resource not found: {uri}")


server.add_request_handler("tools/list", PaginatedRequestParams, _list_tools_handler)
server.add_request_handler("tools/call", CallToolRequestParams, _call_tool_handler)
server.add_request_handler("resources/list", PaginatedRequestParams, _list_resources_handler)
server.add_request_handler("resources/read", ReadResourceRequestParams, _read_resource_handler)


async def main():
    async with stdio_server() as (read_stream, write_stream):
        await server.run(
            read_stream,
            write_stream,
            server.create_initialization_options(),
        )


if __name__ == "__main__":
    asyncio.run(main())
