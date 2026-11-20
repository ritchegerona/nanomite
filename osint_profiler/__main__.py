"""Main entry point for osint_profiler.

Usage:
    python -m osint_profiler <alias1> <alias2> ...
    python -m osint_profiler --file targets.txt
    python -m osint_profiler --help
"""

from __future__ import annotations

import sys

from .cli import main

if __name__ == "__main__":
    sys.exit(main())
