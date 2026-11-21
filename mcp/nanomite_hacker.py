#!/usr/bin/env python3
"""NANOMITE HACKER MODE read-only MCP server (MCP SDK v2.x).

Exposes findings, assessments, and audit log from NANOMITE's HACKER MODE
store. Read-only only: no module execution, no scope changes, no
authorization bypass.
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
    "/Users/ritchegerona/Library/Application Support/nanomite/hacker/findings.db"
)
DEFAULT_AUDIT = Path(
    "/Users/ritchegerona/Library/Application Support/nanomite/hacker/audit.jsonl"
)


def _db_path() -> Path:
    override = os.environ.get("NANOMITE_HACKER_DB")
    return Path(override) if override else DEFAULT_DB


def _open_readonly() -> sqlite3.Connection:
    db = _db_path()
    if not db.exists():
        raise FileNotFoundError(f"hacker findings database not found: {db}")
    uri = f"file:{db}?mode=ro"
    conn = sqlite3.connect(uri, uri=True)
    conn.row_factory = sqlite3.Row
    return conn


TOOL_DEFS = [
    {
        "name": "nanomite_findings_list",
        "description": "List HACKER MODE findings with optional filters (read-only).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "assessment_id": {"type": "string", "description": "Filter by assessment id."},
                "status": {"type": "string", "description": "Filter by finding status (open, triaged, accepted, closed)."},
                "severity": {"type": "string", "description": "Filter by severity label."},
                "limit": {"type": "integer", "description": "Maximum results.", "default": 50},
            },
        },
    },
    {
        "name": "nanomite_assessment_status",
        "description": "List HACKER MODE assessments and their status (read-only).",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "nanomite_audit_read",
        "description": "Read recent HACKER MODE audit log entries (read-only).",
        "inputSchema": {
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "description": "Maximum number of lines to read from the end.", "default": 50},
            },
        },
    },
]

server = Server("nanomite-hacker")


async def _list_tools_handler(ctx, params: PaginatedRequestParams):
    return {"tools": TOOL_DEFS}


async def _call_tool_handler(ctx, params: CallToolRequestParams):
    name = params.name
    arguments = params.arguments or {}

    if name == "nanomite_findings_list":
        assessment_id = arguments.get("assessment_id")
        status = arguments.get("status")
        severity = arguments.get("severity")
        limit = int(arguments.get("limit", 50))
        conn = _open_readonly()
        try:
            sql = (
                "SELECT id, assessment_id, module, operation, target, title, "
                "severity, confidence, status, detail, created_at FROM findings "
                "WHERE 1=1"
            )
            params_sql = []
            if assessment_id:
                sql += " AND assessment_id = ?"
                params_sql.append(assessment_id)
            if status:
                sql += " AND status = ?"
                params_sql.append(status)
            if severity:
                sql += " AND severity = ?"
                params_sql.append(severity)
            sql += " ORDER BY created_at DESC LIMIT ?"
            params_sql.append(limit)
            rows = conn.execute(sql, params_sql).fetchall()
        finally:
            conn.close()
        if not rows:
            return {"content": [{"type": "text", "text": "(no findings)"}]}
        lines = []
        for r in rows:
            detail = (r["detail"] or "")[:120]
            lines.append(
                f"#{r['id']} [{r['status']}] {r['severity']} | "
                f"{r['module']}/{r['operation']} | {r['target']} | {r['title']} | {detail}"
            )
        return {"content": [{"type": "text", "text": "\n".join(lines)}]}

    if name == "nanomite_assessment_status":
        conn = _open_readonly()
        try:
            rows = conn.execute(
                "SELECT id, name, status, cidrs, domains, start, expiry "
                "FROM assessments ORDER BY start DESC"
            ).fetchall()
        finally:
            conn.close()
        if not rows:
            return {"content": [{"type": "text", "text": "(no assessments)"}]}
        lines = [
            f"{r['id']} | {r['name']} | {r['status']} | "
            f"start={r['start']} | expiry={r['expiry']}"
            for r in rows
        ]
        return {"content": [{"type": "text", "text": "\n".join(lines)}]}

    if name == "nanomite_audit_read":
        limit = int(arguments.get("limit", 50))
        audit = DEFAULT_AUDIT
        if not audit.exists():
            return {"content": [{"type": "text", "text": "(no audit log)"}]}
        lines = audit.read_text(errors="replace").splitlines()
        tail = lines[-limit:] if limit > 0 else lines
        return {"content": [{"type": "text", "text": "\n".join(tail)}]}

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
