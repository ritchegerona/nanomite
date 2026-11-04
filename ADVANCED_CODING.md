# Advanced Python Coding Style for Resource-Constrained Projects

## Core Principles

| Principle | Practice |
|-----------|----------|
| **Explicit over implicit** | Type hints on all public functions; no magic globals or hidden side effects. |
| **Immutability by default** | Use `frozen=True`, `slots=True` dataclasses; prefer tuples over lists for fixed data. |
| **Composition over inheritance** | Build small, single-responsibility classes; inject dependencies via constructor arguments. |
| **Lazy evaluation** | Use generators and iterators; avoid loading entire datasets into RAM. |
| **Fail fast, fail loud** | Validate at system boundaries; raise custom exceptions; use EAFP where appropriate. |
| **Observable** | Structured `logging` instead of `print`; log levels (`DEBUG`, `INFO`, `WARNING`, `ERROR`). |
| **Testable** | Pure functions where possible; separate I/O from logic; write pytest cases for every module. |

## Memory-Efficient Patterns (Old Laptops)

1. **`__slots__` + frozen dataclasses** – reduces per-instance memory and prevents accidental attribute creation.
2. **Generators** – `yield` instead of building lists; chain pipelines with `itertools`.
3. **`array.array` / `struct`** – for binary or numeric streams instead of Python lists of ints.
4. **`sys.intern()`** – for repeated string keys (e.g., device types, log tags).
5. **Profile first** – `python -m cProfile -s cumulative script.py`; `tracemalloc` for heap snapshots.

## Recommended Project Layout

```
project_root/
├── pyproject.toml
├── src/
│   └── project/
│       ├── __init__.py
│       ├── core.py        # pure logic, no I/O
│       ├── cli.py         # argument parsing, entry point
│       └── errors.py      # custom exception hierarchy
├── tests/
│   ├── __init__.py
│   └── test_core.py
└── README.md
```

## Minimal `pyproject.toml`

```toml
[project]
name = "nanomite"
version = "0.1.0"
requires-python = ">=3.9"
dependencies = []

[project.optional-dependencies]
dev = ["pytest", "pytest-cov", "mypy", "ruff"]

[build-system]
requires = ["setuptools>=61.0"]
build-backend = "setuptools.build_meta"
```

## Before / After Example

**Naive:**
```python
import subprocess

def get_devices():
    out = subprocess.check_output(["arp", "-a"]).decode()
    devices = []
    for line in out.splitlines():
        if "dynamic" in line:
            parts = line.split()
            devices.append({"ip": parts[0].strip("()"), "mac": parts[3]})
    return devices
```

**Advanced:**
- Frozen slotted dataclass for `NetworkDevice`
- Custom `AuditError` hierarchy
- Context manager for subprocess execution
- Generator `discover_devices()` yields one device at a time
- Type hints, logging, argparse entry point
- Fully covered by pytest

See `src/nanomite/audit_tool.py` and `tests/test_audit_tool.py` for the reference implementation.

## Daily Workflow Checklist

- [ ] Type-check: `python -m mypy src/`
- [ ] Lint: `ruff check src/ tests/`
- [ ] Test: `pytest -q`
- [ ] Profile (when slow): `python -m cProfile -s tottime src/nanomite/cli.py`
- [ ] Memory check: `python -X tracemalloc=1 src/nanomite/cli.py`