# OSINT Profiler Framework

Advanced Open-Source Intelligence (OSINT) profiling and identity resolution framework for authorized security research, CTIA, and penetration testing.

## Project Structure

```
osint_profiler/
├── __main__.py           # Module entry point (python -m osint_profiler)
├── cli/
│   └── __init__.py       # Argparse CLI entry point
├── config/
│   ├── __init__.py
│   ├── settings.py       # Frozen slotted dataclass configuration
│   └── targets_sample.txt # Sample target list
├── core/
│   ├── __init__.py
│   ├── engine.py         # ProfilingEngine - orchestrates the pipeline
│   ├── exceptions.py     # Domain-specific exceptions
│   └── models.py         # Frozen slotted dataclasses (EvidencePoint, TargetProfile, etc.)
├── correlator/
│   └── __init__.py       # Evidence correlation & confidence scoring
├── reporter/
│   └── __init__.py       # Markdown & JSON report generation
├── sources/
│   ├── __init__.py
│   ├── facebook.py       # Facebook OG metadata extraction
│   ├── instagram.py      # Instagram OG metadata extraction
│   ├── telegram.py       # Telegram contact page checking
│   ├── whois.py          # WHOIS domain registration lookup
│   ├── crt_sh.py         # Certificate Transparency log search
│   ├── wayback.py        # Wayback Machine CDX API
│   ├── common_crawl.py   # Common Crawl index search
│   └── darkweb.py        # Dark web / leak forum search (active-only, requires Tor)
└── tests/
    └── test_core.py      # Unit tests (pytest)
```

## Installation

```bash
# System dependencies (Debian/Ubuntu)
sudo apt update && sudo apt install -y whois exiftool tor torsocks curl

# macOS (Homebrew)
brew install whois exiftool tor torsocks

# Python dependencies (if using Telethon for active Telegram)
pip3 install telethon requests beautifulsoup4
```

## Usage

### Profile a single alias
```bash
python -m osint_profiler happygoluckyph
```

### Profile multiple aliases
```bash
python -m osint_profiler siningangmixwithgabi nullsec_philippines ahmdosa
```

### Profile from file
```bash
python -m osint_profiler --file config/targets_sample.txt
```

### Enable active reconnaissance (requires authorization)
```bash
python -m osint_profiler --allow-active happygoluckyph
```

### Custom output directory and format
```bash
python -m osint_profiler --output-dir /path/to/reports --format md --base-name my_investigation
```

### Verbose output
```bash
python -m osint_profiler -vv happygoluckyph
```

## Output

Reports are written to the configured output directory (default: `osint_profiler/output/`):

- `osint_profiling_report.md` — Human-readable Markdown report
- `osint_profiling_report.json` — Machine-readable JSON report

## Confidence Scoring

| Score | Classification | Meaning |
|-------|---------------|---------|
| 0.90-1.00 | Confirmed | Real name directly attributed |
| 0.70-0.89 | High Candidate | Strong evidence, verify before action |
| 0.40-0.69 | Moderate Candidate | Continue investigation |
| 0.10-0.39 | Weak Lead | Low priority |
| 0.00-0.09 | Unresolved | No actionable data |

Formula: `aggregate = 1 - Π(1 - weight_i × confidence_i)`

## Data Sources

| Source | Type | Authentication Required | Data Extracted |
|--------|------|------------------------|----------------|
| Facebook | Passive | No | Page title, ID, followers, locale |
| Instagram | Passive | No | Username, bio, follower count, full name |
| Telegram | Passive | No | Contact page existence, title, description |
| WHOIS | Passive | No | Registrant name, email, country, dates |
| crt.sh | Passive | No | Certificate subjects, issuers |
| Wayback Machine | Passive | No | Archived snapshot URLs |
| Common Crawl | Passive | No | Archived page URLs |
| Dark Web | Active | Yes (Tor + forum registration) | Doxx posts, leak databases |

## Testing

```bash
# Install pytest
pip3 install pytest

# Run all tests
python -m pytest tests/ -v

# Run with coverage
python -m pytest tests/ --cov=. --cov-report=term-missing
```

## OPSEC Notice

This framework is designed for authorized security research only. All active reconnaissance
(social engineering, authenticated access, dark web infiltration) requires explicit written
authorization and legal review before execution. Passive reconnaissance does not interact
directly with target systems beyond public HTTP requests.

## Compliance

Built per DEVELOPMENT_STANDARDS.md:
- Frozen slotted dataclasses for immutability
- Full type hints throughout
- Domain-specific exceptions
- Logging instead of print
- argparse CLI with exit codes
- FOSS-only stack
- pytest-first development
