"""Data source package for osint_profiler.

Each source module exposes a `gather(profile, settings)` function that
returns an updated TargetProfile. Sources may be passive (no auth) or
active (requires authentication / Tor / etc.).
"""

from .facebook import gather as facebook_gather
from .instagram import gather as instagram_gather
from .telegram import gather as telegram_gather
from .whois import gather as whois_gather
from .crt_sh import gather as crt_sh_gather
from .wayback import gather as wayback_gather
from .common_crawl import gather as cc_gather
from .darkweb import gather as darkweb_gather

# Re-export ACTIVE_ONLY flags from each source module
from .facebook import ACTIVE_ONLY as FB_ACTIVE
from .instagram import ACTIVE_ONLY as IG_ACTIVE
from .telegram import ACTIVE_ONLY as TG_ACTIVE
from .whois import ACTIVE_ONLY as WHOIS_ACTIVE
from .crt_sh import ACTIVE_ONLY as CRT_ACTIVE
from .wayback import ACTIVE_ONLY as WB_ACTIVE
from .common_crawl import ACTIVE_ONLY as CC_ACTIVE
from .darkweb import ACTIVE_ONLY as DW_ACTIVE

# Attach ACTIVE_ONLY to each gather function for engine introspection
facebook_gather.ACTIVE_ONLY = FB_ACTIVE
instagram_gather.ACTIVE_ONLY = IG_ACTIVE
telegram_gather.ACTIVE_ONLY = TG_ACTIVE
whois_gather.ACTIVE_ONLY = WHOIS_ACTIVE
crt_sh_gather.ACTIVE_ONLY = CRT_ACTIVE
wayback_gather.ACTIVE_ONLY = WB_ACTIVE
cc_gather.ACTIVE_ONLY = CC_ACTIVE
darkweb_gather.ACTIVE_ONLY = DW_ACTIVE

__all__ = [
    "facebook_gather",
    "instagram_gather",
    "telegram_gather",
    "whois_gather",
    "crt_sh_gather",
    "wayback_gather",
    "cc_gather",
    "darkweb_gather",
    "FB_ACTIVE",
    "IG_ACTIVE",
    "TG_ACTIVE",
    "WHOIS_ACTIVE",
    "CRT_ACTIVE",
    "WB_ACTIVE",
    "CC_ACTIVE",
    "DW_ACTIVE",
]
