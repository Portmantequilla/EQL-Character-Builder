#!/usr/bin/env python
"""import_spell_client_file.py — mine the EQL CLIENT's own spells_us.txt for
authoritative base spell mechanics (mana / cast / recast / recovery / range / target)
and fill gaps our wiki + eqlbuilds data leave open.

This reads files that ship inside every EQL install (the same ones the game itself
loads); it is the spell counterpart of the inventory import — no scraping, no ToS risk.

FIELD LAYOUT (calibrated 2026-07-21 against the eqlbuilds spell_client oracle by joining
on the game spell id; ~100% agreement across 1,058 known spells):
    field[0]  = game spell id        field[8]  = cast time (ms)
    field[1]  = name                 field[9]  = recovery time (ms)
    field[4]  = range                field[10] = recast time (ms)
    field[14] = mana                 field[30] = target type id

MATCHING our spell rows (whose id is the WIKI pageid, not the game id):
  1. eqlbuilds already knows game_id <-> wiki_pageid for ~1,058 spells -> exact (GAME_ID).
  2. otherwise match by canonical NAME; a name unique in the client file is safe
     (validated 902/902 == the known game id) -> NAME_UNIQUE.
  3. a colliding name takes the LOWEST id (the classic-era version EQL uses; validated
     1054/1058) -> NAME_COLLISION (flagged lower confidence).

Idempotent + offline. Re-run after a wiki re-sync (new spells) or a client patch.

Usage:  python scripts/import_spell_client_file.py [--db db/eql.db] [--eql E:/EQL]
"""
import argparse
import collections
import os
import sqlite3
import sys

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DB = os.environ.get("EQL_DB", os.path.join(BASE, "db", "eql.db"))
EQL_DIR = os.environ.get("EQL_GAME_DIR", r"E:\EQL")

# calibrated column indices
F_ID, F_NAME, F_RANGE, F_CAST, F_RECOVERY, F_RECAST, F_MANA, F_TARGET = 0, 1, 4, 8, 9, 10, 14, 30

# the LAST field holds every effect slot, '$'-separated, each 'slot|SPA|base|base2|calc|max'.
# base and max are PRE-COMPUTED by the client (no formula system needed) — validated
# 258/260 exact vs the eqlbuilds decoded dmg/heal (2 misses = regen HoTs). SPA 0 = Hit
# Points (base<0 = damage, base>0 = heal/HP); other SPAs are stat/resist/focus/limit ids.
def parse_effects(last_field):
    out = []
    for grp in last_field.split("$"):
        parts = grp.split("|")
        if len(parts) != 6:
            continue
        try:
            slot, spa, base, base2, calc, mx = (int(x) for x in parts)
        except ValueError:
            continue
        out.append((slot, spa, base, base2, calc, mx))
    return out


def derive_dmg_heal(effs):
    """(dmg_min, dmg_max, heal_min, heal_max) from the SPA-0 (Hit Points) slots."""
    dmin = dmax = hmin = hmax = None
    for _slot, spa, base, _b2, _calc, mx in effs:
        if spa != 0:
            continue
        if base < 0 or (mx and mx < 0):  # damage (negative HP)
            vals = [abs(base)] + ([abs(mx)] if mx else [])
            lo, hi = min(vals), max(vals)
            dmin = lo if dmin is None else min(dmin, lo)
            dmax = hi if dmax is None else max(dmax, hi)
        elif base > 0:  # heal / HP gain
            vals = [base] + ([mx] if mx else [])
            lo, hi = min(vals), max(vals)
            hmin = lo if hmin is None else min(hmin, lo)
            hmax = hi if hmax is None else max(hmax, hi)
    return dmin, dmax, hmin, hmax


def canon(name: str) -> str:
    """lowercase + backtick->apostrophe fold (matches the wiki canonical_name rule)."""
    return name.strip().lower().replace("`", "'")


def load_client_file(path):
    """game id -> fields; and canon name -> sorted list of game ids."""
    by_id, by_name = {}, collections.defaultdict(list)
    with open(path, encoding="latin-1") as fh:
        for line in fh:
            p = line.rstrip("\n").split("^")
            if len(p) <= F_TARGET or not p[F_ID].isdigit():
                continue
            gid = int(p[F_ID])
            by_id[gid] = p
            by_name[canon(p[F_NAME])].append(gid)
    for k in by_name:
        by_name[k].sort()
    return by_id, by_name


def load_landed_text(path):
    """game id -> the 'landed on you' effect text (spells_us_str field 3)."""
    out = {}
    with open(path, encoding="latin-1") as fh:
        for line in fh:
            p = line.rstrip("\n").split("^")
            if len(p) > 3 and p[0].isdigit() and p[3].strip():
                out[int(p[0])] = p[3].strip()
    return out


def as_int(v):
    try:
        n = int(v)
        return n if n not in (-1,) else None  # -1 = "not set" in this format
    except (ValueError, TypeError):
        return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=DB)
    ap.add_argument("--eql", default=EQL_DIR)
    args = ap.parse_args()

    spells_path = os.path.join(args.eql, "spells_us.txt")
    str_path = os.path.join(args.eql, "spells_us_str.txt")
    if not os.path.exists(spells_path):
        sys.exit(f"client spell file not found: {spells_path} (set --eql or EQL_GAME_DIR)")

    by_id, by_name = load_client_file(spells_path)
    landed = load_landed_text(str_path) if os.path.exists(str_path) else {}
    print(f"client file: {len(by_id)} spells, {len(by_name)} distinct names")

    con = sqlite3.connect(args.db)
    cur = con.cursor()
    cur.execute("DROP TABLE IF EXISTS spell_client_file")
    cur.execute("DROP TABLE IF EXISTS spell_client_effect")
    cur.execute(
        """CREATE TABLE spell_client_file (
            spell_id       INTEGER PRIMARY KEY,   -- our wiki pageid
            game_id        INTEGER,
            mana           INTEGER,
            cast_ms        INTEGER,
            recovery_ms    INTEGER,
            recast_ms      INTEGER,
            range          INTEGER,
            target_type_id INTEGER,
            dmg_min        INTEGER,               -- decoded from SPA-0 slots (authoritative)
            dmg_max        INTEGER,
            heal_min       INTEGER,
            heal_max       INTEGER,
            landed_text    TEXT,
            match          TEXT                   -- GAME_ID | NAME_UNIQUE | NAME_COLLISION
        )"""
    )
    # every decoded effect slot (dmg/heal/stat/resist/focus/limit) — base + max
    # pre-computed by the client. spa = the effect id (0=HP, 4..10=STR..INT, 46..50=resists,
    # 124..132=focus types, 134/138/... = focus limits). The durable authoritative-effect
    # store for damage/heal fills, buff values, and precise focus limits.
    cur.execute(
        """CREATE TABLE spell_client_effect (
            spell_id INTEGER,   -- our wiki pageid
            slot     INTEGER,
            spa      INTEGER,
            base     INTEGER,
            base2    INTEGER,
            calc     INTEGER,
            max      INTEGER
        )"""
    )
    cur.execute("CREATE INDEX idx_sce_spell ON spell_client_effect(spell_id)")
    cur.execute("CREATE INDEX idx_sce_spa ON spell_client_effect(spa)")

    # our spells: wiki pageid + canonical name
    spells = [
        (sid, canon(nm))
        for sid, nm in cur.execute(
            "SELECT id, COALESCE(page_title, name) FROM spell WHERE COALESCE(page_title, name) IS NOT NULL"
        )
    ]
    # eqlbuilds bridge: wiki pageid -> known game id (authoritative match)
    known_gid = {}
    if cur.execute("SELECT name FROM sqlite_master WHERE name='spell_client'").fetchone():
        for sid, gid in cur.execute(
            "SELECT spell_id, eqlbuilds_id FROM spell_client WHERE eqlbuilds_id IS NOT NULL"
        ):
            known_gid[sid] = gid

    stats = collections.Counter()
    rows = []
    effect_rows = []
    for sid, name in spells:
        gid = match = None
        if sid in known_gid and known_gid[sid] in by_id:
            gid, match = known_gid[sid], "GAME_ID"
        else:
            ids = by_name.get(name)
            if ids:
                gid = ids[0]
                match = "NAME_UNIQUE" if len(ids) == 1 else "NAME_COLLISION"
        if gid is None:
            stats["unmatched"] += 1
            continue
        p = by_id[gid]
        effs = parse_effects(p[-1]) if p else []
        dmin, dmax, hmin, hmax = derive_dmg_heal(effs)
        rows.append((sid, gid, as_int(p[F_MANA]), as_int(p[F_CAST]), as_int(p[F_RECOVERY]),
                     as_int(p[F_RECAST]), as_int(p[F_RANGE]), as_int(p[F_TARGET]),
                     dmin, dmax, hmin, hmax, landed.get(gid), match))
        for slot, spa, base, base2, calc, mx in effs:
            effect_rows.append((sid, slot, spa, base, base2, calc, mx))
        stats[match] += 1

    cur.executemany(
        "INSERT OR REPLACE INTO spell_client_file VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)", rows
    )
    cur.executemany(
        "INSERT INTO spell_client_effect VALUES (?,?,?,?,?,?,?)", effect_rows
    )
    con.commit()
    print(f"decoded {len(effect_rows)} effect slots across the matched spells")

    print(f"matched {len(rows)} spells -> spell_client_file")
    for k in ("GAME_ID", "NAME_UNIQUE", "NAME_COLLISION", "unmatched"):
        print(f"  {k:16} {stats[k]}")

    # coverage: how many spells gain mana/cast that wiki + eqlbuilds both lacked
    newfill = cur.execute(
        """SELECT COUNT(*) FROM spell s JOIN spell_client_file f ON f.spell_id = s.id
           WHERE s.mana IS NULL AND f.mana IS NOT NULL
             AND NOT EXISTS (SELECT 1 FROM spell_client sc
                             WHERE sc.spell_id = s.id AND sc.mana IS NOT NULL)"""
    ).fetchone()[0]
    print(f"  NEW mana coverage (wiki+eqlbuilds both null): {newfill}")

    # sanity: disagreements between wiki mana and client mana (client is authoritative)
    disagree = cur.execute(
        """SELECT COUNT(*) FROM spell s JOIN spell_client_file f ON f.spell_id = s.id
           WHERE s.mana IS NOT NULL AND f.mana IS NOT NULL AND s.mana <> f.mana"""
    ).fetchone()[0]
    print(f"  wiki/client mana disagreements (kept wiki; client available): {disagree}")
    con.close()


if __name__ == "__main__":
    main()
