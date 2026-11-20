"""Auto-execution pipeline for continuous OSINT profiling."""

from __future__ import annotations
import asyncio
import json
import logging
from datetime import datetime
from pathlib import Path
from typing import Dict, List, Optional

from .config import Profile, Target
from .detector import TargetDetector
from .engine import run_profiling
from .reporter import generate_report

logger = logging.getLogger(__name__)


class ProfilingPipeline:
    """Orchestrates auto-detection, profiling, and reporting for all targets."""
    
    def __init__(
        self,
        workspace_root: str = "/Users/ritchegerona/nanomite",
        output_dir: str = "osint/profiling/reports",
        targets_json: str = "osint/profiling/targets.json",
        skip_sources: Optional[List[str]] = None,
        timeout: float = 30.0,
    ):
        self.workspace_root = Path(workspace_root)
        self.output_dir = self.workspace_root / output_dir
        self.targets_json = self.workspace_root / targets_json
        self.skip_sources = skip_sources or ["common_crawl"]  # Skip slow sources by default
        self.timeout = timeout
        
        self.detector = TargetDetector(str(workspace_root))
        self.profiles: Dict[str, Profile] = {}
    
    async def run_full_pipeline(self) -> Dict[str, Profile]:
        """Run complete auto-detect -> profile -> report pipeline."""
        logger.info("Starting full OSINT profiling pipeline")
        
        # Phase 1: Auto-detect targets
        await self._detect_all_targets()
        
        # Phase 2: Profile all targets
        await self._profile_all_targets()
        
        # Phase 3: Generate reports
        self._generate_all_reports()
        
        # Phase 4: Generate summary
        self._generate_summary()
        
        logger.info("Pipeline complete. Profiled %d targets", len(self.profiles))
        return self.profiles
    
    async def _detect_all_targets(self) -> None:
        """Run all detection methods."""
        # 1. Load from JSON seed registry (primary source for profiling)
        if self.targets_json.exists():
            self.detector.detect_from_targets_json(self.targets_json)
        
        # 2. Scan workspace files (for discovery - stored separately)
        self.detector.detect_from_workspace_files()
        
        # 3. Check memory
        self.detector.detect_from_memory()
        
        # Keep only JSON registry targets for auto-profiling
        json_targets = {}
        for alias, target in self.detector.targets.items():
            detected_from = target.metadata.get("detected_from", "")
            if self.targets_json.exists() and str(self.targets_json) in detected_from:
                json_targets[alias] = target
        
        self.detector.targets = json_targets
        logger.info("Targets queued for profiling: %d (from JSON registry)", len(json_targets))
    
    async def _profile_all_targets(self) -> None:
        """Profile each detected target."""
        targets = self.detector.get_all_targets()
        
        # Get available sources and filter out skipped ones
        from .sources import get_sources
        available_sources = get_sources()
        sources_to_use = [s for s in available_sources if s not in self.skip_sources]
        
        for alias, target in targets.items():
            logger.info("Profiling target: %s", alias)
            try:
                profile = await run_profiling(
                    target=target,
                    sources=sources_to_use,
                    timeout=self.timeout,
                )
                self.profiles[alias] = profile
            except Exception as e:
                logger.error("Failed to profile %s: %s", alias, e)
    
    def _generate_all_reports(self) -> None:
        """Generate individual reports for each profile."""
        self.output_dir.mkdir(parents=True, exist_ok=True)
        
        for alias, profile in self.profiles.items():
            target_dir = self.output_dir / alias
            target_dir.mkdir(exist_ok=True)
            
            # Markdown report
            md_path = target_dir / "profile.md"
            generate_report(profile, md_path, format="markdown")
            
            # JSON report
            json_path = target_dir / "profile.json"
            generate_report(profile, json_path, format="json")
            
            logger.debug("Generated reports for %s", alias)
    
    def _generate_summary(self) -> None:
        """Generate pipeline summary report."""
        summary_path = self.output_dir / "pipeline_summary.json"
        
        summary = {
            "generated_at": datetime.utcnow().isoformat(),
            "total_targets": len(self.profiles),
            "targets": {},
            "tier_distribution": {},
            "unresolved_count": 0,
            "confirmed_count": 0,
        }
        
        tier_counts = {}
        for alias, profile in self.profiles.items():
            tier = profile.tier.value
            tier_counts[tier] = tier_counts.get(tier, 0) + 1
            
            if tier == "unresolved":
                summary["unresolved_count"] += 1
            elif tier == "confirmed":
                summary["confirmed_count"] += 1
            
            summary["targets"][alias] = {
                "alias": profile.target.alias,
                "tier": tier,
                "confidence": profile.aggregate_confidence,
                "real_name_candidates": profile.real_name_candidates,
                "evidence_count": len(profile.evidence),
                "location_profile": profile.location_profile,
            }
        
        summary["tier_distribution"] = tier_counts
        
        with open(summary_path, 'w') as f:
            json.dump(summary, f, indent=2)
        
        logger.info("Generated pipeline summary at %s", summary_path)
        
        # Also generate Markdown summary
        md_path = self.output_dir / "pipeline_summary.md"
        self._generate_markdown_summary(summary, md_path)
    
    def _generate_markdown_summary(self, summary: dict, path: Path) -> None:
        """Generate human-readable Markdown summary."""
        lines = [
            "# OSINT Profiling Pipeline Summary",
            f"**Generated:** {summary['generated_at']}",
            f"**Total Targets:** {summary['total_targets']}",
            f"**Confirmed:** {summary['confirmed_count']} | **Unresolved:** {summary['unresolved_count']}",
            "",
            "## Tier Distribution",
        ]
        
        for tier, count in summary['tier_distribution'].items():
            lines.append(f"- **{tier}**: {count}")
        
        lines.append("")
        lines.append("## Target Details")
        lines.append("")
        
        for alias, info in summary['targets'].items():
            lines.append(f"### {alias}")
            lines.append(f"- **Tier:** {info['tier']}")
            lines.append(f"- **Confidence:** {info['confidence']:.2f}")
            lines.append(f"- **Evidence Count:** {info['evidence_count']}")
            
            if info['real_name_candidates']:
                lines.append(f"- **Real Name Candidates:** {', '.join(info['real_name_candidates'])}")
            
            if info['location_profile']:
                loc = info['location_profile']
                if loc.get('country'):
                    lines.append(f"- **Location:** {loc['country']} (confidence: {loc.get('confidence', 0):.2f})")
            
            lines.append("")
        
        path.write_text('\n'.join(lines))


async def run_pipeline(
    workspace_root: str = "/Users/ritchegerona/nanomite",
    output_dir: str = "osint/profiling/reports",
    targets_json: str = "osint/profiling/targets.json",
    skip_sources: Optional[List[str]] = None,
    timeout: float = 30.0,
) -> Dict[str, Profile]:
    """Convenience function to run the full pipeline."""
    pipeline = ProfilingPipeline(
        workspace_root=workspace_root,
        output_dir=output_dir,
        targets_json=targets_json,
        skip_sources=skip_sources,
        timeout=timeout,
    )
    return await pipeline.run_full_pipeline()