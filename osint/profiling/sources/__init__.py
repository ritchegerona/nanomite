"""Passive data source gatherers for OSINT profiling."""

from __future__ import annotations
import asyncio
import logging
from typing import Callable, Dict, List, Optional

from ..config import Evidence, Target

logger = logging.getLogger(__name__)

# Source registry: name -> (gather_func, is_active_only)
_SOURCE_REGISTRY: Dict[str, tuple[Callable, bool]] = {}


def register_source(name: str, active_only: bool = False):
    """Decorator to register a passive gatherer."""
    def decorator(func: Callable) -> Callable:
        _SOURCE_REGISTRY[name] = (func, active_only)
        return func
    return decorator


def get_sources(include_active_only: bool = False) -> Dict[str, Callable]:
    """Get registered gatherer functions."""
    if include_active_only:
        return {name: func for name, (func, _) in _SOURCE_REGISTRY.items()}
    return {name: func for name, (func, active) in _SOURCE_REGISTRY.items() if not active}


def get_active_sources() -> Dict[str, Callable]:
    """Get only active-only sources (require auth)."""
    return {name: func for name, (func, active) in _SOURCE_REGISTRY.items() if active}


# Auto-import and register all source modules
def _auto_register_sources() -> None:
    """Import all source modules to trigger @register_source decorators."""
    try:
        from . import facebook  # noqa: F401
        from . import instagram  # noqa: F401
        from . import telegram  # noqa: F401
        from . import defacement  # noqa: F401
        # common_crawl is slow, skip by default
        logger.debug("Registered %d passive sources", len(_SOURCE_REGISTRY))
    except Exception as e:
        logger.warning("Failed to auto-register sources: %s", e)


# Register on import
_auto_register_sources()


async def gather_all(
    target: Target,
    sources: Optional[List[str]] = None,
    include_active_only: bool = False,
    timeout: float = 30.0,
) -> List[Evidence]:
    """Run all registered gatherers for a target concurrently."""
    available = get_sources(include_active_only)
    
    if sources:
        available = {k: v for k, v in available.items() if k in sources}
    
    logger.info("Running %d sources for target %s", len(available), target.alias)
    
    async def run_source(name: str, func: Callable) -> List[Evidence]:
        try:
            return await asyncio.wait_for(func(target), timeout=timeout)
        except asyncio.TimeoutError:
            logger.warning("Source %s timed out for %s", name, target.alias)
            return []
        except Exception as e:
            logger.error("Source %s failed for %s: %s", name, target.alias, e)
            return []
    
    tasks = [run_source(name, func) for name, func in available.items()]
    results = await asyncio.gather(*tasks)
    
    all_evidence = []
    for evidence_list in results:
        all_evidence.extend(evidence_list)
    
    logger.info("Collected %d evidence items for %s", len(all_evidence), target.alias)
    return all_evidence