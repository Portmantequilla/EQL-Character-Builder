#!/usr/bin/env python3
"""make_dist_db.py — build the SLIM wiki database that ships with the installer.

db/eql.db is the working mirror: it keeps the raw wikitext so the parsers can be
re-run offline. Users don't need any of that. This produces app/src-tauri/resources/
wiki.db with the dev-only bulk stripped and the file VACUUMed, so the installer stays
small while the app behaves identically.

Run: python scripts/make_dist_db.py
"""
import os
import shutil
import sqlite3
import sys

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(BASE, "db", "eql.db")
OUT_DIR = os.path.join(BASE, "app", "src-tauri", "resources")
OUT = os.path.join(OUT_DIR, "wiki.db")

# Columns holding raw wikitext: only the importer needs them.
# NOTE: spell.where_to_obtain_raw is KEPT — the reparse scripts read it, but so does
# nothing at runtime; still small, and dropping it would break a re-run of the reparse
# pipeline against a slimmed DB. spell.raw_wikitext is the big one.
BLANK_COLUMNS = [("spell", "raw_wikitext"), ("items", "raw_statsblock")]
# Tables the running app never queries (kept in the dev DB for future parsers).
# spell_source + spell_item_source now FEED the `?` acquisition popup (spell_info) —
# they must SHIP. class_vendor_directory is a small reference table kept for a future view.
DROP_TABLES = ["spell_categories", "item_categories"]


def mb(path):
    return os.path.getsize(path) / 1048576


def main():
    if not os.path.exists(SRC):
        sys.exit(f"missing {SRC} — run the sync first")
    os.makedirs(OUT_DIR, exist_ok=True)
    shutil.copyfile(SRC, OUT)
    before = mb(OUT)

    con = sqlite3.connect(OUT)
    for table, col in BLANK_COLUMNS:
        try:
            con.execute(f"UPDATE {table} SET {col} = NULL")
        except sqlite3.OperationalError as e:
            print(f"  skip {table}.{col}: {e}")
    for t in DROP_TABLES:
        con.execute(f"DROP TABLE IF EXISTS {t}")
    con.commit()
    con.execute("VACUUM")
    con.commit()

    # sanity: the app's core tables must survive
    checks = {
        "items": 10000, "spell": 1800, "mobs": 6000, "drops": 30000,
        "buff_line_member": 400, "item_slots": 7000, "race_base_stats": 100,
        "spell_class_level": 1900, "item_effect": 1400,
        # user research imports — re-run their import scripts after any re-sync
        "pet_summon_estimate": 90,
        # eqlbuilds.com snapshot (scripts/import_eqlbuilds.py)
        "stance": 8, "invocation": 9, "class_skill": 500, "spell_client": 900,
        # spell acquisition — the `?` popup (reparse_spells + reparse_spell_sources +
        # import_spell_locations); ships now (was dropped, so shipped popups were empty)
        "spell_source": 7000, "spell_item_source": 1000,
        # EQL client spells_us.txt mine (import_spell_client_file.py) — fills mana/cast gaps
        "spell_client_file": 1700,
        # community HP/mana base curves (import_stat_estimator.py) — the ESTIMATOR model
        "class_base_curve": 1500,
    }
    print(f"\n{OUT}")
    print(f"  {before:.1f} MB -> {mb(OUT):.1f} MB")
    bad = []
    for t, floor in checks.items():
        n = con.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
        flag = "" if n >= floor else "  <-- TOO FEW"
        if n < floor:
            bad.append(t)
        print(f"  {t:20} {n:6}{flag}")
    con.close()
    if bad:
        sys.exit(f"\nFAIL: distribution db is missing data in {bad}")
    print("\nOK — bundled by tauri.conf.json (resources) and copied to "
          "%LOCALAPPDATA%/EQLBuilder/wiki.db on first run.")


if __name__ == "__main__":
    main()
