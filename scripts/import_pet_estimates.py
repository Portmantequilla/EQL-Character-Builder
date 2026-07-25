#!/usr/bin/env python3
"""Import the user's pet-summon research workbook into wiki.db.

Source: EQL_Pet_Summons_and_Tier_Upgrades.xlsx ("Pet Summons" sheet) — 108 summons
across 7 classes with estimated fixed pet levels, rough EQL HP (legacy ~doubled), and
base max hits, each row carrying a confidence label. Core rule (Official, 7/7 notes):
each tier attempts +1 pet level capped at player level - 1, and ONLY actual levels
gained grant stats (+6% HP, +1 base damage, +5 skill points per level).

Writes the `pet_summon_estimate` table keyed by spell_id (name-matched). The app's
loader fills spell_pet_summon NULLs from it — wiki-tested values always win, so the
few conflicts (e.g. Bone Walk 9 tested vs 7 estimated) keep the tested number.

Idempotent; re-run after any wiki re-sync (like extract_item_dims.py):
    python scripts/import_pet_estimates.py [path-to-xlsx]
Default xlsx path: raw/EQL_Pet_Summons_and_Tier_Upgrades.xlsx (falls back to the
OneDrive Desktop original if the raw/ copy is absent).
"""
import os
import re
import sqlite3
import sys
from pathlib import Path

import openpyxl

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "db" / "eql.db"
DEFAULT_XLSX = ROOT / "raw" / "EQL_Pet_Summons_and_Tier_Upgrades.xlsx"
# override with EQL_PET_XLSX or pass the path as the first CLI arg
FALLBACK_XLSX = Path(os.environ.get("EQL_PET_XLSX", str(DEFAULT_XLSX)))


def norm(s: str) -> str:
    return re.sub(r"\s+", " ", str(s).replace("`", "'")).strip().lower()


def main() -> None:
    xlsx = Path(sys.argv[1]) if len(sys.argv) > 1 else (
        DEFAULT_XLSX if DEFAULT_XLSX.exists() else FALLBACK_XLSX
    )
    if not xlsx.exists():
        sys.exit(f"workbook not found: {xlsx}")
    wb = openpyxl.load_workbook(xlsx, data_only=True)
    ws = wb["Pet Summons"]

    con = sqlite3.connect(DB)
    spell_ids = {
        norm(nm): sid
        for sid, nm in con.execute(
            "SELECT id, COALESCE(page_title, name) FROM spell"
        )
    }
    con.execute("DROP TABLE IF EXISTS pet_summon_estimate")
    con.execute(
        """CREATE TABLE pet_summon_estimate (
             spell_id INTEGER PRIMARY KEY,
             spell_name TEXT NOT NULL,
             class TEXT, pet_type TEXT, pet_classes TEXT,
             base_pet_level INTEGER,
             hp_est INTEGER, max_hit_est INTEGER,
             duration TEXT, availability TEXT,
             confidence TEXT, notes TEXT,
             source TEXT NOT NULL DEFAULT 'user research workbook 2026-07-16')"""
    )

    matched = unmatched = 0
    conflicts = []
    db_levels = dict(
        con.execute("SELECT spell_id, base_pet_level FROM spell_pet_summon")
    )
    # a spell shared by two classes (NEC/SHD line) appears twice with per-class
    # estimates; keep the HIGHER-level variant — all three wiki-TESTED levels
    # (Leering Corpse 5, Bone Walk 9, Restless Bones 16) match the higher estimate
    best: dict[int, tuple] = {}
    for r in ws.iter_rows(min_row=2, values_only=True):
        (cls, spell, _reqlvl, era, ptype, pclasses, base_lvl, _llo, _lhi, _lhp,
         hp_est, hit_est, dur, conf, notes) = r[:15]
        if not spell:
            continue
        sid = spell_ids.get(norm(spell))
        if sid is None:
            unmatched += 1
            print(f"  no spell match: {spell}")
            continue
        matched += 1
        row = (sid, str(spell), cls, ptype, pclasses,
               int(base_lvl) if base_lvl is not None else None,
               int(hp_est) if hp_est is not None else None,
               int(hit_est) if hit_est is not None else None,
               dur, era, conf, notes)
        prev = best.get(sid)
        if prev is None or (row[5] or 0) > (prev[5] or 0):
            best[sid] = row
    for sid, row in best.items():
        con.execute(
            """INSERT INTO pet_summon_estimate
               (spell_id, spell_name, class, pet_type, pet_classes, base_pet_level,
                hp_est, max_hit_est, duration, availability, confidence, notes)
               VALUES (?,?,?,?,?,?,?,?,?,?,?,?)""",
            row,
        )
        tested = db_levels.get(sid)
        if tested is not None and row[5] is not None and int(tested) != int(row[5]):
            conflicts.append((row[1], tested, row[5]))

    con.commit()
    n = con.execute("SELECT COUNT(*) FROM pet_summon_estimate").fetchone()[0]
    print(f"imported {n} estimates ({matched} matched, {unmatched} unmatched)")
    if conflicts:
        print("level conflicts (wiki-tested value KEPT, estimate ignored):")
        for s, t, e in conflicts:
            print(f"  {s}: tested {t} vs estimate {e}")
    con.close()


if __name__ == "__main__":
    main()
