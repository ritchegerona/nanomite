#!/usr/bin/env python3
"""NANOMITE build MCP server (MCP SDK v2.x).

Runs read-only build and test commands inside the NANOMITE workspace.
"""

import asyncio
import subprocess
from pathlib import Path

from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import (
    CallToolRequestParams,
    PaginatedRequestParams,
    TextContent,
)

WORKSPACE_ROOT = Path("/Users/ritchegerona/nanomite").resolve()

ALLOWED_COMMANDS = {
    "nanomite_cargo_check": ["cargo", "check"],
    "nanomite_cargo_test_lib": ["cargo", "test", "--lib"],
    "nanomite_cargo_test_all": ["cargo", "test"],
    "nanomite_cargo_clippy": ["cargo", "clippy", "--all-targets", "--", "-D", "warnings"],
    "nanomite_cargo_fmt_check": ["cargo", "fmt", "--check"],
    "nanomite_pytest": ["python3", "-m", "pytest"],
}

TOOL_DEFS = [
    {
        "name": "nanomite_cargo_check",
        "description": "Run `cargo check` on the NANOMITE workspace (read-only).",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "nanomite_cargo_test_lib",
        "description": "Run `cargo test --lib` on NANOMITE (784 tests, ~5s, fully offline).",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "nanomite_cargo_test_all",
        "description": "Run the full `cargo test` suite including integration targets.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "nanomite_cargo_clippy",
        "description": "Run `cargo clippy --all-targets -- -D warnings`.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "nanomite_cargo_fmt_check",
        "description": "Run `cargo fmt --check` to verify formatting.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "nanomite_pytest",
        "description": "Run Python pytest for NANOMITE's Python tooling.",
        "inputSchema": {"type": "object", "properties": {}},
    },
]

server = Server("nanomite-build")


async def _list_tools_handler(ctx, params: PaginatedRequestParams):
    return {"tools": TOOL_DEFS}


async def _call_tool_handler(ctx, params: CallToolRequestParams):
    name = params.name
    arguments = params.arguments or {}

    if name not in ALLOWED_COMMANDS:
        raise ValueError(f"unknown command: {name}")

    cmd = ALLOWED_COMMANDS[name]
    try:
        result = subprocess.run(
            cmd,
            cwd=str(WORKSPACE_ROOT),
            capture_output=True,
            text=True,
            timeout=300,
        )
    except subprocess.TimeoutExpired:
        return {
            "content": [{"type": "text", "text": "command timed out: " + " ".join(cmd)}],
            "isError": True,
        }
    except FileNotFoundError:
        return {
            "content": [{"type": "text", "text": "binary not found: " + cmd[0]}],
            "isError": True,
        }

    output = result.stdout
    if result.stderr:
        output += "\n--- STDERR ---\n" + result.stderr
    output += f"\n--- exit code: {result.returncode} ---"
    return {"content": [{"type": "text", "text": output}]}


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
