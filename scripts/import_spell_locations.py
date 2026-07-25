#!/usr/bin/env python3
"""Import the authoritative EQLWiki spell purchase-location dataset into wiki.db.

Source: raw/eql_spells_full.json (a complete Category:Spells extraction, 2026-07-17:
1943 spells, 13205 purchase locations, class_vendor_directory, with revision ids).
Each purchase_location carries the REAL merchant, zone, area and coordinates keyed by
spell_page_id (== our spell.id) — far better than the SpellWhereTable regex parse.

source_kind mapping:
  "Explicit spell page"       -> VENDOR       (merchant listed on the spell's own page)
  "Class representative vendor"-> CLASS_VENDOR (the class guild vendor list; a fallback
                                 imported ONLY for spells with no explicit vendor, so the
                                 popup isn't buried under 8 guild cities per spell)

Replaces spell_source VENDOR + CLASS_VENDOR rows; leaves DROP/QUEST/RESEARCH/NOTE (loot/
quest/research/free-text) from the sync + reparse untouched. Adds a class_source column
and a class_vendor_directory table. Idempotent; run after reparse_spell_sources.py.
"""
import json
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "db" / "eql.db"
SRC = ROOT / "raw" / "eql_spells_full.json"

ALLOWED = "('VENDOR','DROP','QUEST','RESEARCH','CLASS_VENDOR','NOTE','UNKNOWN')"


def ensure_schema(con: sqlite3.Connection) -> None:
    cols = {r[1] for r in con.execute("PRAGMA table_info(spell_source)")}
    ddl = con.execute(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='spell_source'"
    ).fetchone()[0]
    if "class_source" not in cols or "CLASS_VENDOR" not in ddl:
        con.execute("DROP TABLE IF EXISTS spell_source_old")  # clean any prior half-run
        con.execute("ALTER TABLE spell_source RENAME TO spell_source_old")
        con.execute(
            f"""CREATE TABLE spell_source (
                 id INTEGER PRIMARY KEY,
                 spell_id INTEGER NOT NULL,
                 source_type TEXT NOT NULL CHECK (source_type IN {ALLOWED}),
                 zone_name TEXT, npc_name TEXT, area TEXT, loc TEXT,
                 class_source TEXT, raw_text TEXT, source_revision INTEGER)"""
        )
        old_cols = {r[1] for r in con.execute("PRAGMA table_info(spell_source_old)")}
        common = [c for c in
                  ("id", "spell_id", "source_type", "zone_name", "npc_name", "area",
                   "loc", "class_source", "raw_text", "source_revision")
                  if c in old_cols]
        con.execute(
            f"INSERT INTO spell_source ({','.join(common)}) "
            f"SELECT {','.join(common)} FROM spell_source_old"
        )
        con.execute("DROP TABLE spell_source_old")


def main() -> None:
    if not SRC.exists():
        raise SystemExit(f"missing {SRC} — copy eql_spells_full.json into raw/ first")
    data = json.load(open(SRC, encoding="utf-8"))
    con = sqlite3.connect(DB)
    ensure_schema(con)

    our_ids = {r[0] for r in con.execute("SELECT id FROM spell")}

    # spells that DO have an explicit-page vendor: their class-representative rows are
    # redundant noise (the class directory), so we skip those (Data Notes: class vendor
    # is the "fallback when the spell page only links Class Spell Vendors")
    explicit_ids = {
        p["spell_page_id"] for p in data["purchase_locations"]
        if p.get("source_kind") == "Explicit spell page" and p.get("spell_page_id") in our_ids
    }

    con.execute("DELETE FROM spell_source WHERE source_type IN ('VENDOR','CLASS_VENDOR')")

    n_expl = n_class = skipped = 0
    for p in data["purchase_locations"]:
        sid = p.get("spell_page_id")
        if sid not in our_ids:
            skipped += 1
            continue
        kind = p.get("source_kind")
        if kind == "Explicit spell page":
            stype = "VENDOR"
            n_expl += 1
        elif kind == "Class representative vendor":
            if sid in explicit_ids:
                continue  # explicit vendor known — skip the class-directory fallback
            stype = "CLASS_VENDOR"
            n_class += 1
        else:
            continue
        con.execute(
            "INSERT INTO spell_source(spell_id, source_type, zone_name, npc_name, area, \
             loc, class_source) VALUES (?,?,?,?,?,?,?)",
            (sid, stype, p.get("zone") or None, p.get("merchant") or None,
             p.get("area") or None, p.get("coordinates") or None,
             (p.get("class") or None) if stype == "CLASS_VENDOR" else None),
        )

    # class vendor directory (dict keyed by class -> {vendors: [...]}) for a future
    # "class spell vendors" reference view
    con.execute("DROP TABLE IF EXISTS class_vendor_directory")
    con.execute(
        "CREATE TABLE class_vendor_directory (class TEXT, zone TEXT, merchant TEXT, \
         area TEXT, coordinates TEXT)"
    )
    cvd = data.get("class_vendor_directory", {})
    for cls, entry in cvd.items():
        for v in entry.get("vendors", []):
            con.execute(
                "INSERT INTO class_vendor_directory VALUES (?,?,?,?,?)",
                (cls, v.get("zone"), v.get("merchant"), v.get("area"), v.get("coordinates")),
            )

    # provenance
    con.execute("DROP TABLE IF EXISTS spell_locations_provenance")
    con.execute("CREATE TABLE spell_locations_provenance (generated_at TEXT, "
                "spell_count INTEGER, purchase_location_count INTEGER, source_category TEXT)")
    con.execute(
        "INSERT INTO spell_locations_provenance VALUES (?,?,?,?)",
        (data.get("generated_at"), data.get("spell_count"),
         data.get("purchase_location_count"), data.get("source_category")),
    )

    con.commit()
    covered = con.execute(
        "SELECT COUNT(DISTINCT spell_id) FROM spell_source WHERE source_type IN ('VENDOR','CLASS_VENDOR')"
    ).fetchone()[0]
    print(f"explicit vendor rows {n_expl}, class-vendor fallback rows {n_class}, "
          f"skipped (not in our db) {skipped}")
    print(f"spells with vendor locations: {covered}; class_vendor_directory: "
          f"{con.execute('SELECT COUNT(*) FROM class_vendor_directory').fetchone()[0]}")
    print(f"provenance: {data.get('generated_at')} ({data.get('purchase_location_count')} locations)")
    con.close()


if __name__ == "__main__":
    main()
