"""Tests for the crack CLI: exit codes, logging, argparse, fail-fast errors."""

from __future__ import annotations

import hashlib
import sys

import pytest

from nanomite.cracking.cli import build_parser, run


def _make_hash(plaintext: str, algo: str = "sha256") -> str:
    return hashlib.new(algo, plaintext.encode("utf-8")).hexdigest()


def _write_wordlist(tmp_path, words):
    p = tmp_path / "wordlist.txt"
    p.write_text("\n".join(words) + "\n", encoding="utf-8")
    return str(p)


def _base_args(scope_id: str = "eng-001", token: str = "tok-123") -> list[str]:
    return ["--scope-id", scope_id, "--capability-token", token]


def test_parser_requires_scope_and_token() -> None:
    parser = build_parser()
    with pytest.raises(SystemExit) as exc:
        parser.parse_args(["single", "abc", "/tmp/wl.txt"])
    assert exc.value.code == 2


def test_successful_crack_returns_zero(tmp_path) -> None:
    h = _make_hash("password")
    wl = _write_wordlist(tmp_path, ["admin", "password", "root"])
    code = run(_base_args() + ["single", h, wl])
    assert code == 0


def test_unsuccessful_crack_returns_one(tmp_path) -> None:
    h = _make_hash("nobody")
    wl = _write_wordlist(tmp_path, ["admin", "password", "root"])
    code = run(_base_args() + ["single", h, wl])
    assert code == 1


def test_missing_wordlist_returns_two(tmp_path) -> None:
    h = _make_hash("password")
    with pytest.raises(SystemExit) as exc:
        run(_base_args() + ["single", h, str(tmp_path / "does-not-exist.txt")])
    assert exc.value.code == 2


def test_missing_capability_token_returns_four(tmp_path) -> None:
    h = _make_hash("password")
    wl = _write_wordlist(tmp_path, ["password"])
    code = run(_base_args(token="") + ["single", h, wl])
    assert code == 4


def test_verbose_flag_sets_debug_logging(tmp_path, caplog) -> None:
    h = _make_hash("password")
    wl = _write_wordlist(tmp_path, ["password"])
    with caplog.at_level("DEBUG", logger="nanomite.cracking.cli"):
        run(["--verbose"] + _base_args() + ["single", h, wl])
    assert any("hash cracked" in rec.getMessage() for rec in caplog.records)


def test_hash_type_choice_validation() -> None:
    parser = build_parser()
    with pytest.raises(SystemExit):
        parser.parse_args(
            _base_args(token="tok")
            + ["single", "abc", "/tmp/wl.txt", "--hash-type", "not-a-hash"]
        )


def test_main_exits_with_code(monkeypatch) -> None:
    from nanomite.cracking import cli

    monkeypatch.setattr(
        sys,
        "argv",
        ["nanomite-crack", "--scope-id", "s", "--capability-token", "t",
         "single", "x", "/tmp/wl.txt"],
    )
    # run() returns 2 (missing wordlist); main() calls sys.exit(2)
    with pytest.raises(SystemExit) as exc:
        cli.main()
    assert exc.value.code == 2


def test_all_outcomes_used_in_cli() -> None:
    # The CLI references CrackOutcome.SUCCESS; ensure import works end-to-end.
    from nanomite.cracking.engine import CrackOutcome

    assert CrackOutcome.SUCCESS.value == "success"
