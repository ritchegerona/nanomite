"""Tests for the generator-based wordlist module."""

from __future__ import annotations

import pytest

from nanomite.cracking.errors import CrackError
from nanomite.cracking.wordlist import WordlistConfig, count_words, stream_words


def _write(tmp_path, content: str):
    p = tmp_path / "wl.txt"
    p.write_text(content, encoding="utf-8")
    return str(p)


def test_stream_yields_one_at_a_time(tmp_path) -> None:
    path = _write(tmp_path, "alpha\nbeta\ngamma\n")
    words = list(stream_words(WordlistConfig(path=path)))
    assert words == ["alpha", "beta", "gamma"]


def test_stream_is_lazy_generator(tmp_path) -> None:
    path = _write(tmp_path, "a\nb\nc\n")
    it = stream_words(WordlistConfig(path=path))
    assert hasattr(it, "__next__")
    first = next(it)
    assert first == "a"


def test_min_max_len_filters(tmp_path) -> None:
    path = _write(tmp_path, "a\nab\nabc\nabcd\n")
    words = list(
        stream_words(WordlistConfig(path=path, min_len=2, max_len=3))
    )
    assert words == ["ab", "abc"]


def test_dedupe(tmp_path) -> None:
    path = _write(tmp_path, "x\ny\nx\nz\ny\n")
    words = list(stream_words(WordlistConfig(path=path, dedupe=True)))
    assert words == ["x", "y", "z"]


def test_no_dedupe(tmp_path) -> None:
    path = _write(tmp_path, "x\ny\nx\n")
    words = list(stream_words(WordlistConfig(path=path, dedupe=False)))
    assert words == ["x", "y", "x"]


def test_lowercase(tmp_path) -> None:
    path = _write(tmp_path, "HeLLo\nWORLD\n")
    words = list(stream_words(WordlistConfig(path=path, lowercase=True)))
    assert words == ["hello", "world"]


def test_cap_limits(tmp_path) -> None:
    path = _write(tmp_path, "1\n2\n3\n4\n5\n")
    words = list(stream_words(WordlistConfig(path=path, cap=3)))
    assert words == ["1", "2", "3"]


def test_empty_lines_skipped(tmp_path) -> None:
    path = _write(tmp_path, "\n\nfoo\n\nbar\n\n")
    words = list(stream_words(WordlistConfig(path=path)))
    assert words == ["foo", "bar"]


def test_count_words(tmp_path) -> None:
    path = _write(tmp_path, "a\nb\nc\n")
    assert count_words(WordlistConfig(path=path)) == 3


def test_missing_file_raises(tmp_path) -> None:
    missing = tmp_path / "nope.txt"
    with pytest.raises(CrackError):
        list(stream_words(WordlistConfig(path=str(missing))))
