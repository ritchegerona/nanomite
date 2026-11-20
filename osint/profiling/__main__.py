"""Main entry point for OSINT Profiling Framework."""

from .cli import main

if __name__ == "__main__":
    import asyncio
    import sys
    sys.exit(asyncio.run(main()))