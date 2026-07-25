#!/usr/bin/env python3
"""Recover the acquisition rows the spell sync's SpellWhereTable parser missed.

The wiki explicitly lists where every spell is obtained, but three formats never made
it into spell_source (audit 2026-07-17):
  * `<ul><li>[[Beastlord#Spell Vendors|Beastlord Spell Vendors]]` lists (74 spells)
      -> one CLASS_VENDOR row per class ("sold by that class's guild spell vendors")
  * `{{Enchanter Recipe| X | 22}}` + `{{Recipe Component| ... }}` blocks (37 spells)
      -> one RESEARCH row carrying the trivial + component list
  * free text ("Shaman spell merchant", "* [[Sleeper's Tomb]] (Named mobs)") (11)
      -> one NOTE row shown verbatim
Also: junk UNKNOWN rows ('}}', '<ul>') are deleted, and non-junk UNKNOWN rows
("In Book of Dark Bindings:", "Yield: X") become NOTE rows so the popup shows them.

Incremental and idempotent — never touches existing VENDOR/DROP/QUEST/RESEARCH rows.
Run after any spell re-sync:  python scripts/reparse_spell_sources.py
"""
import re
import sqlite3
from pathlib import Path

DB = Path(__file__).resolve().parent.parent / "db" / "eql.db"

CLASS_VENDOR_RE = re.compile(r"\[\[(\w[\w ]*?)#Spell Vendors\|", re.I)
RECIPE_RE = re.compile(r"\{\{(\w[\w ]*?) Recipe\s*\|\s*([^|}]+?)\s*\|\s*(\d+)\s*\}\}")
COMPONENT_RE = re.compile(r"\{\{Recipe Component\s*\|\s*([^|}]+?)\s*(?:\|\s*(\d+)\s*)?\}\}")
WIKILINK_RE = re.compile(r"\[\[(?:[^|\]]*\|)?([^\]]+)\]\]")


def clean_text(raw: str) -> str:
    """Strip wiki markup to display text."""
    t = WIKILINK_RE.sub(r"\1", raw)
    t = re.sub(r"\[https?://\S+(?:\s+([^\]]+))?\]", r"\1", t)  # external links -> label
    t = re.sub(r"<[^>]+>", " ", t)
    t = re.sub(r"\{\{[^}]*\}\}", " ", t)
    t = t.replace("*", " ").replace("#", " ")
    return re.sub(r"\s+", " ", t).strip()


def widen_check_constraint(con: sqlite3.Connection) -> None:
    """The sync's table CHECKs source_type to the original five values — rebuild it
    once to admit CLASS_VENDOR/NOTE (SQLite can't alter a CHECK in place)."""
    ddl = con.execute(
        "SELECT sql FROM sqlite_master WHERE type='table' AND name='spell_source'"
    ).fetchone()[0]
    if "CLASS_VENDOR" in ddl:
        return  # already widened
    con.execute("DROP TABLE IF EXISTS spell_source_old")  # clean any prior half-run
    con.execute("ALTER TABLE spell_source RENAME TO spell_source_old")
    con.execute(
        """CREATE TABLE spell_source (
             id INTEGER PRIMARY KEY,
             spell_id INTEGER NOT NULL,
             source_type TEXT NOT NULL CHECK (source_type IN
               ('VENDOR','DROP','QUEST','RESEARCH','CLASS_VENDOR','NOTE','UNKNOWN')),
             zone_name TEXT, npc_name TEXT, area TEXT, loc TEXT,
             raw_text TEXT, source_revision INTEGER)"""
    )
    con.execute(
        "INSERT INTO spell_source SELECT * FROM spell_source_old"
    )
    con.execute("DROP TABLE spell_source_old")


def main() -> None:
    con = sqlite3.connect(DB)
    widen_check_constraint(con)
    # ---- 1. delete junk UNKNOWN rows ('}}', '<ul>', empty) ----
    junk = con.execute(
        "DELETE FROM spell_source WHERE source_type='UNKNOWN' \
         AND (raw_text IS NULL OR LENGTH(TRIM(raw_text)) <= 4)"
    ).rowcount

    # ---- 2. non-junk UNKNOWN rows -> NOTE (visible in the popup) ----
    promoted = con.execute(
        "UPDATE spell_source SET source_type='NOTE' WHERE source_type='UNKNOWN'"
    ).rowcount

    # ---- 3. parse the missed formats for spells with NO useful rows yet ----
    gap = con.execute(
        """SELECT s.id, COALESCE(s.page_title, s.name), s.where_to_obtain_raw
           FROM spell s
           WHERE s.where_to_obtain_raw IS NOT NULL AND s.where_to_obtain_raw != ''
           AND NOT EXISTS (SELECT 1 FROM spell_source src
                           WHERE src.spell_id = s.id
                           AND src.source_type NOT IN ('UNKNOWN','NOTE'))"""
    ).fetchall()

    n_cv = n_res = n_note = 0
    for sid, _name, raw in gap:
        classes = CLASS_VENDOR_RE.findall(raw)
        recipes = RECIPE_RE.findall(raw)
        handled = False
        for cls in dict.fromkeys(classes):  # dedupe, keep order
            con.execute(
                "INSERT INTO spell_source(spell_id, source_type, zone_name, npc_name, \
                 area, loc, raw_text) VALUES (?,?,?,?,?,?,?)",
                (sid, "CLASS_VENDOR", None, f"{cls} spell vendors", None, None, None),
            )
            n_cv += 1
            handled = True
        for cls, _spellname, trivial in recipes:
            comps = [c[0].strip() for c in COMPONENT_RE.findall(raw)]
            detail = f"{cls} research, trivial {trivial}"
            comp_text = "; components: " + ", ".join(comps) if comps else ""
            con.execute(
                "INSERT INTO spell_source(spell_id, source_type, zone_name, npc_name, \
                 area, loc, raw_text) VALUES (?,?,?,?,?,?,?)",
                (sid, "RESEARCH", None, None, detail, None,
                 (detail + comp_text) if comp_text else None),
            )
            n_res += 1
            handled = True
        if not handled:
            text = clean_text(raw)
            if text:
                con.execute(
                    "INSERT INTO spell_source(spell_id, source_type, zone_name, \
                     npc_name, area, loc, raw_text) VALUES (?,?,?,?,?,?,?)",
                    (sid, "NOTE", None, None, None, None, text[:400]),
                )
                n_note += 1

    con.commit()
    covered = con.execute(
        """SELECT COUNT(DISTINCT s.id) FROM spell s JOIN spell_source src ON src.spell_id=s.id
           WHERE s.is_npc_only=0 AND src.source_type != 'UNKNOWN'"""
    ).fetchone()[0]
    remaining = con.execute(
        """SELECT COUNT(*) FROM spell s WHERE s.is_npc_only=0
           AND s.where_to_obtain_raw IS NOT NULL AND s.where_to_obtain_raw != ''
           AND NOT EXISTS (SELECT 1 FROM spell_source src WHERE src.spell_id=s.id
                           AND src.source_type != 'UNKNOWN')"""
    ).fetchone()[0]
    print(f"junk deleted {junk}; UNKNOWN->NOTE {promoted}; "
          f"new: class-vendor {n_cv}, research {n_res}, note {n_note}")
    print(f"spells with useful acquisition rows: {covered}; wiki-text-without-rows remaining: {remaining}")
    con.close()


if __name__ == "__main__":
    main()
