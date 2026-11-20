"""Auto-detection of investigation targets from multiple sources."""

from __future__ import annotations
import json
import logging
import re
from pathlib import Path
from typing import Dict, List, Optional

import httpx

from .config import DetectionResult, Target, load_targets_from_json

logger = logging.getLogger(__name__)


class TargetDetector:
    """Auto-detect targets from workspace, memory, and external sources."""
    
    def __init__(self, workspace_root: str = "/Users/ritchegerona/nanomite"):
        self.workspace_root = Path(workspace_root)
        self.targets: Dict[str, Target] = {}
    
    def detect_from_targets_json(self, path: str | Path) -> Dict[str, Target]:
        """Load targets from JSON seed registry."""
        targets = load_targets_from_json(path)
        self.targets.update(targets)
        logger.info("Loaded %d targets from %s", len(targets), path)
        return targets
    
    def detect_from_workspace_files(self) -> Dict[str, Target]:
        """Scan workspace for potential target identifiers."""
        targets = {}
        
        # Patterns to search for
        patterns = {
            "telegram": r'@([a-zA-Z0-9_]{5,32})',
            "email": r'([a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,})',
            "facebook_id": r'(?:facebook\.com/|fb\.com/|id=)(\d{10,})',
            "instagram": r'instagram\.com/([a-zA-Z0-9_.]{3,30})',
            "domain_ph": r'([a-zA-Z0-9-]+\.ph)',
        }
        
        # Search common file types
        extensions = {'.json', '.txt', '.md', '.py', '.js', '.html', '.csv'}
        
        for file_path in self.workspace_root.rglob('*'):
            if file_path.suffix.lower() in extensions and file_path.is_file():
                try:
                    content = file_path.read_text(encoding='utf-8', errors='ignore')
                    for source, pattern in patterns.items():
                        matches = re.findall(pattern, content)
                        for match in matches:
                            # Create a minimal target from each unique identifier
                            alias = self._normalize_alias(match, source)
                            if alias and alias not in targets:
                                targets[alias] = Target(
                                    alias=alias,
                                    metadata={"detected_from": str(file_path), "source": source}
                                )
                except Exception as e:
                    logger.debug("Failed to scan %s: %s", file_path, e)
        
        logger.info("Detected %d targets from workspace files", len(targets))
        self.targets.update(targets)
        return targets
    
    def detect_from_memory(self) -> Dict[str, Target]:
        """Detect targets from persistent memory (nanomite memory system)."""
        targets = {}
        try:
            # Try to import and use nanomite memory
            import sys
            sys.path.insert(0, self.workspace_root)
            from src.nanomite.memory import memory_search
            
            # Search for OSINT-related memories
            results = memory_search(query="target alias", tags=["osint", "target", "investigation"])
            
            for result in results:
                # Extract potential aliases from memory content
                aliases = self._extract_aliases_from_text(result.get("content", ""))
                for alias in aliases:
                    if alias not in targets:
                        targets[alias] = Target(
                            alias=alias,
                            metadata={"detected_from": "memory", "memory_id": result.get("id")}
                        )
        except Exception as e:
            logger.debug("Memory detection failed: %s", e)
        
        logger.info("Detected %d targets from memory", len(targets))
        self.targets.update(targets)
        return targets
    
    def detect_from_telegram_channels(self, channel_list: List[str]) -> Dict[str, Target]:
        """Detect targets from known Telegram channel lists."""
        targets = {}
        # This would require active Telegram API access
        # Placeholder for future implementation
        logger.info("Telegram channel detection not yet implemented")
        return targets
    
    def _normalize_alias(self, raw: str, source: str) -> Optional[str]:
        """Normalize a raw identifier into a target alias."""
        raw = raw.strip().lower()
        
        # Skip common false positives
        skip_patterns = [
            r'^http', r'^www\.', r'\.com$', r'\.org$', r'\.net$',
            r'^@?telegram$', r'^@?channel$', r'^@?group$',
            r'^[a-z]{1,3}$', r'^\d+$',
        ]
        
        for pattern in skip_patterns:
            if re.match(pattern, raw):
                return None
        
        # Clean based on source
        if source == "telegram":
            return raw.lstrip('@')
        elif source == "email":
            return raw.split('@')[0]  # Use local part as alias
        elif source == "facebook_id":
            return f"fb_{raw}" if raw.isdigit() else raw
        elif source == "instagram":
            return raw.lstrip('@')
        elif source == "domain_ph":
            return raw.replace('.ph', '')
        
        return raw if len(raw) >= 3 else None
    
    def _extract_aliases_from_text(self, text: str) -> List[str]:
        """Extract potential aliases from free text."""
        aliases = []
        
        # Look for @mentions
        telegram_matches = re.findall(r'@([a-zA-Z0-9_]{5,32})', text)
        aliases.extend([m for m in telegram_matches if not m.startswith('http')])
        
        # Look for "alias:" or "handle:" patterns
        handle_matches = re.findall(r'(?:alias|handle|username)[:=]\s*([a-zA-Z0-9_]{3,32})', text, re.IGNORECASE)
        aliases.extend(handle_matches)
        
        return list(set(aliases))
    
    def get_all_targets(self) -> Dict[str, Target]:
        """Get all detected targets."""
        return self.targets
    
    def save_targets(self, path: str | Path) -> None:
        """Save detected targets to JSON."""
        from .config import save_targets_to_json
        save_targets_to_json(self.targets, path)
        logger.info("Saved %d targets to %s", len(self.targets), path)