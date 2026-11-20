#!/usr/bin/env python3
"""
Nanomite OSINT Identity Mapping & Doxxing Framework — Main Orchestrator

Authorized security research only. Targets must be owned by the user or
covered by explicit written permission.

Demonstrates the full pipeline on the DNH Klammer → Rodel Lablab case
using evidence gathered from prior web research.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

# Ensure local package imports work when run as a script
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from osint.identity_mapper import IdentityMapper, EvidenceRecord
from osint.social_enum import SocialEnumerator
from osint.email_harvest import EmailHarvester
from osint.metadata_extractor import MetadataExtractor
from osint.correlator import Correlator
from osint.report_generator import ReportGenerator


def build_dnh_klammer_case() -> None:
    """End-to-end demonstration: DNH Klammer → Rodel Lablab."""
    print("=" * 60)
    print("NANOMITE OSINT — Identity Mapping & Doxxing Framework")
    print("Case: DNH Klammer → Rodel Lablab (authorized research demo)")
    print("=" * 60)

    mapper = IdentityMapper()
    correlator = Correlator(mapper)
    reporter = ReportGenerator(output_dir="report")

    # ---------------------------------------------------------------
    # Step 1: Create identity candidate for the alias
    # ---------------------------------------------------------------
    print("\n[1/6] Creating identity candidate for 'DNH Klammer'...")
    cand = mapper.create_candidate("DNH Klammer")

    # ---------------------------------------------------------------
    # Step 2: Add evidence from verified OSINT sources
    # ---------------------------------------------------------------
    print("[2/6] Ingesting verified OSINT evidence...")

    mapper.add_evidence(
        alias="DNH Klammer",
        source="Quantum Security Group blog (quantum-sec-group.blogspot.com)",
        quote="Greetz: DNH Klammer - Rodel Lablab",
        confidence=0.95,
        tags=["direct_alias_name"],
        source_url="https://quantum-sec-group.blogspot.com",
        date_observed="2026-07-03",
        real_name="Rodel Lablab",
    )

    mapper.add_evidence(
        alias="DNH Klammer",
        source="Quantum Security Group blog (DICT egov post)",
        quote="Special Greetingz: DNH_Klammer and to the DeathNote Hackers International!",
        confidence=0.80,
        tags=["affiliation_mention", "direct_alias_name"],
        source_url="https://quantum-sec-group.blogspot.com",
        date_observed="2026-08-06",
    )

    mapper.add_evidence(
        alias="DNH Klammer",
        source="Newsbytes.PH",
        quote="A hacktivist using the name DNH Klammer has called on the Office of the Ombudsman to examine an alleged P79.95-million CICC contract... awarded to Bowman Technologies Inc.",
        confidence=0.85,
        tags=["corroborating_news"],
        source_url="https://newsbytes.ph/2026/09/24/hacktivist-seeks-ombudsman-probe-into-alleged-p80-m-cicc-software-deal/",
        date_observed="2026-09-24",
    )

    mapper.add_evidence(
        alias="DNH Klammer",
        source="POLITIKO",
        quote="DNH Klammer, a known figure in Philippine cybersecurity circles, claimed online that a 'hefty amount' of Wi-Fi access points had suddenly appeared around the Senate complex.",
        confidence=0.80,
        tags=["corroborating_news"],
        source_url="https://politiko.com.ph/rogue-routers-or-false-alarm-dict-checks-alleged-wi-fi-threats-around-senate/",
        date_observed="2026-05-16",
    )

    mapper.add_evidence(
        alias="DNH Klammer",
        source="Facebook (DNHklammerv1)",
        quote="I truly believe Rodel can fit any hole and any role this government can offer. We've seen Rodel's expertise in this field, from compshop enjoyer all the way up to DOTr.",
        confidence=0.70,
        tags=["social_profile_match"],
        source_url="https://facebook.com/DNHklammerv1",
    )

    mapper.add_evidence(
        alias="DNH Klammer",
        source="Facebook (DNHklammerv1)",
        quote="#Nag labas si Plasabas #Plataporma #RODEL4DICT --CTLYN + Klammer",
        confidence=0.65,
        tags=["social_profile_match"],
        source_url="https://facebook.com/DNHklammerv1",
    )

    # ---------------------------------------------------------------
    # Step 3: Merge known alias variants
    # ---------------------------------------------------------------
    print("[3/6] Merging alias variants...")
    mapper.create_candidate("DNH.klammer")
    mapper.create_candidate("DNHklammerv1")
    mapper.create_candidate("0xKlammerV2")
    mapper.create_candidate("DNH_Klammer")

    # Merge all variants into the primary identity
    for variant in ["DNH.klammer", "DNHklammerv1", "0xKlammerV2", "DNH_Klammer"]:
        if mapper.get_candidate(variant):
            mapper.merge_alias("DNH Klammer", variant)

    # ---------------------------------------------------------------
    # Step 4: Social media enumeration (candidate URLs)
    # ---------------------------------------------------------------
    print("[4/6] Enumerating social media candidates...")
    enumerator = SocialEnumerator()
    social_candidates = enumerator.enumerate("DNH.klammer")
    social_candidates.extend(enumerator.enumerate("Rodel Lablab"))
    social_candidates.extend(enumerator.enumerate("Rodel Lab"))

    # Add observed profiles from prior research as verified
    observed = [
        {
            "platform": "facebook",
            "username": "DNHklammerv1",
            "url": "https://facebook.com/DNHklammerv1",
            "display_name": "DNH Klammer",
            "confidence": 0.8,
        },
        {
            "platform": "facebook",
            "username": "0xKlammerV2",
            "url": "https://facebook.com/0xKlammerV2",
            "display_name": "DNH Klammer",
            "confidence": 0.8,
        },
        {
            "platform": "instagram",
            "username": "laboneterodel",
            "url": "https://instagram.com/laboneterodel/",
            "display_name": "Rodel Lab",
            "is_private": True,
            "confidence": 0.6,
        },
        {
            "platform": "facebook",
            "username": "laboneterodel",
            "url": "https://facebook.com/laboneterodel/",
            "display_name": "Rodel Lab",
            "confidence": 0.6,
        },
        {
            "platform": "tryhackme",
            "username": "DNH.klammer",
            "url": "https://tryhackme.com/p/DNH.klammer",
            "confidence": 0.7,
        },
    ]
    social_candidates.extend(observed)

    # Deduplicate by (platform, username)
    seen_social = set()
    unique_social = []
    for sp in social_candidates:
        # Support both SocialProfile dataclass and dict
        if hasattr(sp, "platform"):
            key = (sp.platform, sp.username)
            sp_dict = sp.to_dict()
        else:
            key = (sp["platform"], sp["username"])
            sp_dict = sp
        if key not in seen_social:
            seen_social.add(key)
            unique_social.append(sp_dict)

    # ---------------------------------------------------------------
    # Step 5: Build correlated profile
    # ---------------------------------------------------------------
    print("[5/6] Correlating identity profile...")
    profile = correlator.build_profile(
        alias="DNH Klammer",
        social_profiles=unique_social,
        emails=[],  # none harvested in this demo run
        metadata_findings=[],  # no files analyzed in this demo run
    )
    profile.confidence = correlator.score_confidence(profile)

    print(f"\n{'='*60}")
    print("CORRELATED IDENTITY PROFILE")
    print(f"{'='*60}")
    print(f"Primary alias : {profile.primary_alias}")
    print(f"Real name     : {profile.real_name}")
    print(f"Confidence    : {profile.confidence:.2f} / 1.00")
    print(f"Evidence items: {profile.evidence_count}")
    print(f"Aliases       : {', '.join(profile.aliases)}")
    print(f"Affiliations  : {', '.join(profile.affiliations) or 'none'}")
    print(f"Employers     : {', '.join(profile.possible_employers) or 'none'}")
    print(f"Social profiles: {len(profile.social_profiles)}")
    for sp in profile.social_profiles:
        print(f"  - {sp['platform']}: {sp['username']} ({sp['url']}) conf={sp['confidence']:.2f}")
    print(f"{'='*60}")

    # ---------------------------------------------------------------
    # Step 6: Generate reports
    # ---------------------------------------------------------------
    print("[6/6] Generating reports...")
    md_path = reporter.generate_markdown(profile, filename="dnh_klammer_identity_report.md")
    json_path = reporter.generate_json(profile, filename="dnh_klammer_identity_profile.json")
    mapper.export_json("report/dnh_klammer_mapper_export.json")

    print(f"\nReports written:")
    print(f"  Markdown : {md_path}")
    print(f"  JSON     : {json_path}")
    print(f"  Mapper   : report/dnh_klammer_mapper_export.json")

    print("\nFramework ready for authorized OSINT operations.")


if __name__ == "__main__":
    build_dnh_klammer_case()