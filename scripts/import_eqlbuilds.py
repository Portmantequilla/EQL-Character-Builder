#!/usr/bin/env python3
"""Import the eqlbuilds.com snapshot (via the MIT-licensed everquest-legends-mcp repo)
into wiki.db. Source JSONs live in raw/eqlbuilds/ (manifest.json carries provenance:
bundle SHA, extractedAt, wiki revision).

What lands where:
  stances.json / invocations.json -> stance / invocation tables (the combat modes)
  classes.json skillList          -> class_skill (per class: trained_at + cap)
  classes.json spellList          -> spell_client (client-extracted spell data joined
                                     to our spell table BY NAME: mana/cast/recast,
                                     resolved description, REAL damage/heal ranges —
                                     the fix for stale legacy wiki effect lines)
  general + class abilities       -> fills aa.cost_json where ours is incomplete ('?')
                                     and eqlbuilds' costStatus is 'known' AND max_rank
                                     matches (never overwrites complete data)

CAUTION: client-side data — the server can still override values, so treat as
PARTIALLY_VERIFIED. Idempotent; re-run after wiki re-syncs and eqlbuilds refreshes:
    python scripts/import_eqlbuilds.py
"""
import json
import re
import sqlite3
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DB = ROOT / "db" / "eql.db"
SRC = ROOT / "raw" / "eqlbuilds"

CLASS_ABBR = {
    "warrior": "WAR", "cleric": "CLR", "paladin": "PAL", "ranger": "RNG",
    "shadowKnight": "SHD", "druid": "DRU", "monk": "MNK", "bard": "BRD",
    "rogue": "ROG", "shaman": "SHM", "necromancer": "NEC", "wizard": "WIZ",
    "magician": "MAG", "enchanter": "ENC", "beastlord": "BST", "berserker": "BER",
}


def norm(s: str) -> str:
    return re.sub(r"\s+", " ", str(s).replace("`", "'")).strip().lower()


def hit_point_range(effects: list) -> tuple:
    """(dmg_min, dmg_max, heal_min, heal_max) from the first Hit-points effect.
    base==0 with a nonzero max classifies by the MAX's sign — regen-style HoTs ship
    as {baseValue: 0, maxValue: 9} ("Hit points: 0 to 9") and are heals."""
    for e in effects or []:
        if e.get("name") != "Hit points":
            continue
        base = e.get("baseValue") or 0
        mx = e.get("maxValue") or 0
        if base == 0 and mx == 0:
            continue  # no usable range on this line
        if base < 0 or (base == 0 and mx < 0):  # damage: base -700, max 808 -> 700..808
            lo = -base
            hi = abs(mx) if mx != 0 else lo
            return (min(lo, hi), max(lo, hi), None, None)
        # healing (base > 0, or base == 0 with mx > 0)
        lo = base
        hi = mx if mx > 0 else lo
        return (None, None, min(lo, hi), max(lo, hi))
    return (None, None, None, None)


def main() -> None:
    con = sqlite3.connect(DB)
    manifest = json.load(open(SRC / "manifest.json", encoding="utf-8"))

    # ---- provenance ----
    con.execute("DROP TABLE IF EXISTS eqlbuilds_provenance")
    con.execute(
        "CREATE TABLE eqlbuilds_provenance (source TEXT, source_url TEXT, "
        "bundle_sha256 TEXT, extracted_at TEXT, wiki_revision_id INTEGER)"
    )
    con.execute(
        "INSERT INTO eqlbuilds_provenance VALUES (?,?,?,?,?)",
        (manifest.get("source"), manifest.get("sourceUrl"),
         manifest.get("bundleSha256"), manifest.get("extractedAt"),
         manifest.get("wikiRevisionId")),
    )

    # ---- stances + invocations (the combat modes) ----
    for table, fname in (("stance", "stances.json"), ("invocation", "invocations.json")):
        rows = json.load(open(SRC / fname, encoding="utf-8"))
        con.execute(f"DROP TABLE IF EXISTS {table}")
        con.execute(
            f"CREATE TABLE {table} (id TEXT PRIMARY KEY, name TEXT NOT NULL, "
            f"message TEXT, description TEXT)"
        )
        for r in rows:
            con.execute(
                f"INSERT INTO {table} VALUES (?,?,?,?)",
                (r["id"], r["name"], r.get("message"), r.get("description")),
            )
        print(f"{table}: {len(rows)}")

    classes = json.load(open(SRC / "classes.json", encoding="utf-8"))

    # ---- per-class skills (trained_at + cap; the BEST_OF combine's data) ----
    con.execute("DROP TABLE IF EXISTS class_skill")
    con.execute(
        "CREATE TABLE class_skill (class_abbr TEXT NOT NULL, skill_id INTEGER, "
        "name TEXT NOT NULL, trained_at INTEGER, cap INTEGER, description TEXT, "
        "PRIMARY KEY (class_abbr, name))"
    )
    n_skills = 0
    for cname, c in classes.items():
        abbr = CLASS_ABBR.get(cname)
        if not abbr:
            print(f"  unknown class key: {cname}")
            continue
        for sk in c.get("skillList", []):
            con.execute(
                "INSERT OR REPLACE INTO class_skill VALUES (?,?,?,?,?,?)",
                (abbr, sk.get("id"), sk["name"], sk.get("trainedAt"),
                 sk.get("cap"), sk.get("description") or None),
            )
            n_skills += 1
    print(f"class_skill: {n_skills}")

    # ---- client spell data, joined to our spell table by name ----
    spell_ids = {
        norm(nm): sid
        for sid, nm in con.execute("SELECT id, COALESCE(page_title, name) FROM spell")
    }
    con.execute("DROP TABLE IF EXISTS spell_client")
    con.execute(
        """CREATE TABLE spell_client (
             spell_id INTEGER PRIMARY KEY,   -- our wiki spell id (name-joined)
             eqlbuilds_id INTEGER, name TEXT NOT NULL,
             mana INTEGER, cast_ms INTEGER, recovery_ms INTEGER, recast_ms INTEGER,
             duration_ticks INTEGER, range INTEGER, skill TEXT, target_type_id INTEGER,
             icon_id INTEGER, resolved_description TEXT,
             dmg_min INTEGER, dmg_max INTEGER, heal_min INTEGER, heal_max INTEGER,
             effects_json TEXT)"""
    )
    uniq: dict = {}
    for c in classes.values():
        for s in c.get("spellList", []):
            uniq.setdefault(norm(s["name"]), s)
    matched = unmatched = 0
    for key, s in uniq.items():
        sid = spell_ids.get(key)
        if sid is None:
            unmatched += 1
            continue
        matched += 1
        dmg_min, dmg_max, heal_min, heal_max = hit_point_range(s.get("effects"))
        con.execute(
            """INSERT OR REPLACE INTO spell_client VALUES
               (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
            (sid, s.get("id"), s["name"], s.get("manaCost"), s.get("castTimeMs"),
             s.get("recoveryTimeMs"), s.get("recastTimeMs"), s.get("durationTicks"),
             s.get("range"), s.get("skill"), s.get("targetTypeId"), s.get("iconId"),
             s.get("resolvedDescription"), dmg_min, dmg_max, heal_min, heal_max,
             json.dumps(s.get("effects") or [])),
        )
    print(f"spell_client: {matched} matched, {unmatched} unmatched (kept out)")

    # ---- AA cost fill: only where ours is INCOMPLETE and theirs is KNOWN ----
    allaa: dict = {}
    for c in classes.values():
        for a in c.get("alternateAbilityList", []):
            allaa[(norm(a["name"]), (a.get("category") or "").upper())] = a
    try:
        gen = json.load(open(SRC / "general-abilities.json", encoding="utf-8"))
        for a in gen:
            allaa.setdefault((norm(a["name"]), (a.get("category") or "").upper()), a)
    except FileNotFoundError:
        pass
    # provenance column so eqlbuilds-sourced fills stay refreshable: wiki-complete rows
    # are never touched, but rows WE filled get re-applied from the CURRENT bundle every
    # run (a corrected upstream cost must not be locked in forever). import_aa.py
    # rebuilds the table without this column — re-add it each run.
    aa_cols = {r[1] for r in con.execute("PRAGMA table_info(aa)")}
    if "cost_source" not in aa_cols:
        con.execute("ALTER TABLE aa ADD COLUMN cost_source TEXT")
        con.execute("UPDATE aa SET cost_source='wiki' WHERE cost_complete=1")
    filled = skipped = 0
    for aid, aname, acat, amax, cj, cc, src in con.execute(
        "SELECT id, name, category, max_rank, cost_json, cost_complete, cost_source FROM aa"
    ).fetchall():
        if cc and src != "eqlbuilds":
            continue  # wiki-complete — never touch
        a = allaa.get((norm(aname), (acat or "").upper())) or allaa.get((norm(aname), "GENERAL"))
        if a is None or a.get("costStatus") != "known":
            continue
        costs = a.get("rankCosts") or []
        if len(costs) != amax or any(x is None for x in costs):
            skipped += 1
            continue
        con.execute(
            "UPDATE aa SET cost_json=?, cost_complete=1, cost_source='eqlbuilds' WHERE id=?",
            (json.dumps([int(x) for x in costs]), aid),
        )
        filled += 1
    print(f"aa cost fill: {filled} filled/refreshed, {skipped} skipped (rank-count mismatch)")

    con.commit()
    print(f"provenance: extracted {manifest.get('extractedAt')} from {manifest.get('source')}")
    con.close()


if __name__ == "__main__":
    main()
