#!/usr/bin/env python3
"""NANOMITE memory MCP server (MCP SDK v2.x).

Read-only queries against NANOMITE's SQLite memory store.
"""

import asyncio
import os
import sqlite3
from pathlib import Path

from mcp.server import Server
from mcp.server.stdio import stdio_server
from mcp.types import (
    CallToolRequestParams,
    PaginatedRequestParams,
)

DEFAULT_DB = Path(
    "/Users/ritchegerona/Library/Application Support/nanomite/memory/nanomite.db"
)


def _db_path() -> Path:
    override = os.environ.get("NANOMITE_MEMORY_DB")
    return Path(override) if override else DEFAULT_DB


def _open_readonly() -> sqlite3.Connection:
    db = _db_path()
    if not db.exists():
        raise FileNotFoundError(f"memory database not found: {db}")
    uri = f"file:{db}?mode=ro"
    conn = sqlite3.connect(uri, uri=True)
    conn.row_factory = sqlite3.Row
    return conn


TOOL_DEFS = [
    {
        "name": "nanomite_memory_search",
        "description": "Search NANOMITE persistent memory for facts, decisions, and preferences.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search term (matched against content and summary)."},
                "category": {"type": "string", "description": "Filter by category (core, user, project, episodic, decision, session, task)."},
                "project": {"type": "string", "description": "Filter by project key."},
                "limit": {"type": "integer", "description": "Maximum results.", "default": 20},
            },
            "required": ["query"],
        },
    },
    {
        "name": "nanomite_memory_get",
        "description": "Fetch one NANOMITE memory record by its exact id.",
        "inputSchema": {
            "type": "object",
            "properties": {"id": {"type": "string", "description": "Memory record id."}},
            "required": ["id"],
        },
    },
    {
        "name": "nanomite_memory_list",
        "description": "List NANOMITE memory records with optional filters.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "category": {"type": "string", "description": "Filter by category."},
                "project": {"type": "string", "description": "Filter by project key."},
                "limit": {"type": "integer", "description": "Maximum results.", "default": 50},
            },
        },
    },
]

server = Server("nanomite-memory")


async def _list_tools_handler(ctx, params: PaginatedRequestParams):
    return {"tools": TOOL_DEFS}


async def _call_tool_handler(ctx, params: CallToolRequestParams):
    name = params.name
    arguments = params.arguments or {}

    if name == "nanomite_memory_search":
        query = arguments["query"]
        category = arguments.get("category")
        project = arguments.get("project")
        limit = int(arguments.get("limit", 20))
        conn = _open_readonly()
        try:
            sql = (
                "SELECT id, category, content, summary, tags, project, "
                "created_at, importance FROM memories "
                "WHERE (content LIKE ? OR summary LIKE ?)"
            )
            params_sql = [f"%{query}%", f"%{query}%"]
            if category:
                sql += " AND category = ?"
                params_sql.append(category)
            if project:
                sql += " AND project = ?"
                params_sql.append(project)
            sql += " ORDER BY importance DESC, updated_at DESC LIMIT ?"
            params_sql.append(limit)
            rows = conn.execute(sql, params_sql).fetchall()
        finally:
            conn.close()
        if not rows:
            return {"content": [{"type": "text", "text": "(no matching memories)"}]}
        lines = []
        for r in rows:
            content = r["content"] or ""
            summary = r["summary"] or ""
            display = summary or content[:200]
            lines.append(
                f"[{r['id']}] {r['category']} | {r['project'] or '-'} | "
                f"imp={r['importance']} | {display}"
            )
        return {"content": [{"type": "text", "text": "\n".join(lines)}]}

    if name == "nanomite_memory_get":
        mid = arguments["id"]
        conn = _open_readonly()
        try:
            row = conn.execute(
                "SELECT id, category, content, summary, tags, project, "
                "created_at, updated_at, importance, sensitivity FROM memories "
                "WHERE id = ?",
                (mid,),
            ).fetchone()
        finally:
            conn.close()
        if not row:
            return {
                "content": [{"type": "text", "text": f"memory not found: {mid}"}],
                "isError": True,
            }
        lines = [
            f"id: {row['id']}",
            f"category: {row['category']}",
            f"project: {row['project'] or '-'}",
            f"importance: {row['importance']}",
            f"sensitivity: {row['sensitivity']}",
            f"created: {row['created_at']}",
            f"updated: {row['updated_at']}",
            f"summary: {row['summary'] or '-'}",
            f"tags: {row['tags'] or '-'}",
            f"content: {row['content']}",
        ]
        return {"content": [{"type": "text", "text": "\n".join(lines)}]}

    if name == "nanomite_memory_list":
        category = arguments.get("category")
        project = arguments.get("project")
        limit = int(arguments.get("limit", 50))
        conn = _open_readonly()
        try:
            sql = (
                "SELECT id, category, summary, project, importance, updated_at "
                "FROM memories WHERE 1=1"
            )
            params_sql = []
            if category:
                sql += " AND category = ?"
                params_sql.append(category)
            if project:
                sql += " AND project = ?"
                params_sql.append(project)
            sql += " ORDER BY updated_at DESC LIMIT ?"
            params_sql.append(limit)
            rows = conn.execute(sql, params_sql).fetchall()
        finally:
            conn.close()
        if not rows:
            return {"content": [{"type": "text", "text": "(no memories)"}]}
        lines = [
            f"{r['id']} | {r['category']} | {r['project'] or '-'} | "
            f"imp={r['importance']} | {r['updated_at']} | {r['summary'] or '-'}"
            for r in rows
        ]
        return {"content": [{"type": "text", "text": "\n".join(lines)}]}

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
