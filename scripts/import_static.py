#!/usr/bin/env python3
"""import_static.py — race base stats + per-class stat modifiers from the wiki
"Statistics" page into db/eql.db.

Cache-first and idempotent: the raw wikitext is cached under raw/fixtures/static/;
once cached, re-runs are fully OFFLINE. Network use is polite (reuses the API
constant + User-Agent from eql_wiki_sync.py, ~0.5s pacing) and hard-capped at
MAX_REQUESTS requests.

Tables written (never DROPped):
  race(id, name)                      -- 15 playable races, plan Group-12 stable ids
  race_base_stats(race_id, stat, value)
  class_stat_mod(class_id, str, sta, agi, dex, wis, intel, cha)  -- ADDITIVE mods

Both tables live on Statistics (verified revid 153407, 2026-07-13):
  'eoTable2' = Race x STR/STA/AGI/DEX/WIS/INT/CHA/Total  (Kerra row is flagged
  "assumed from EQ Live Vah Shir" on the wiki -- printed, not modeled)
  'eoTable3' = Class x additive +5/+10/+15 mods (blank cell = 0)

Usage:  python scripts/import_static.py [--db db/eql.db] [--refetch]
"""
import os, re, sys, json, time, argparse, sqlite3

SCRIPTS = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPTS)
from eql_wiki_sync import API, UA, CLASS_ID, CLASS_NAME2ABBR  # noqa: E402

BASE = os.path.dirname(SCRIPTS)
DB = os.environ.get("EQL_DB", os.path.join(BASE, "db", "eql.db"))
FIXDIR = os.path.join(BASE, "raw", "fixtures", "static")
MAX_REQUESTS = 15
PACE_SECONDS = 0.5

# plan Group-12 seed: ids are CONSTANTS (docs/character-builder-plan.md)
RACE_SEED = [(1, "Human"), (2, "Barbarian"), (3, "Erudite"), (4, "Wood Elf"),
             (5, "High Elf"), (6, "Dark Elf"), (7, "Half Elf"), (8, "Dwarf"),
             (9, "Troll"), (10, "Ogre"), (11, "Halfling"), (12, "Gnome"),
             (13, "Iksar"), (14, "Froglok"), (15, "Kerra")]
RACE_ALIASES = {"Half-Elf": "Half Elf", "Half elf": "Half Elf",
                "Woodelf": "Wood Elf", "Highelf": "High Elf", "Darkelf": "Dark Elf"}
STATS = ["STR", "STA", "AGI", "DEX", "WIS", "INT", "CHA"]

_request_count = 0

def fetch_wikitext(title):
    """Cached wikitext for a page title; fetches (politely) only on cache miss."""
    global _request_count
    os.makedirs(FIXDIR, exist_ok=True)
    safe = re.sub(r"[^A-Za-z0-9_-]", "_", title.replace(" ", "_"))
    cache = os.path.join(FIXDIR, safe + ".wikitext")
    meta = os.path.join(FIXDIR, safe + ".meta.json")
    if os.path.exists(cache):
        revid = None
        if os.path.exists(meta):
            revid = json.load(open(meta)).get("revid")
        print("  [cache] %s (revid %s)" % (title, revid))
        return open(cache, encoding="utf-8").read(), revid
    if _request_count >= MAX_REQUESTS:
        raise RuntimeError("hard request limit (%d) reached" % MAX_REQUESTS)
    import requests  # only the network path needs it
    _request_count += 1
    time.sleep(PACE_SECONDS)
    s = requests.Session()
    s.headers["User-Agent"] = UA
    r = s.get(API, params={"action": "query", "prop": "revisions",
                           "rvslots": "main", "rvprop": "content|ids",
                           "titles": title, "format": "json",
                           "formatversion": "2"}, timeout=60)
    r.raise_for_status()
    page = r.json()["query"]["pages"][0]
    if page.get("missing"):
        print("  [MISSING] %s" % title)
        return None, None
    rev = page["revisions"][0]
    wt, revid = rev["slots"]["main"]["content"], rev["revid"]
    open(cache, "w", encoding="utf-8").write(wt)
    json.dump({"title": title, "revid": revid}, open(meta, "w"))
    print("  [fetched] %s (revid %s, %d bytes) -> cached" % (title, revid, len(wt)))
    return wt, revid

# ------------------------------------------------------------------- wikitext
LINK = re.compile(r"\[\[([^\]|]+)(?:\|[^\]]*)?\]\]")

def strip_cell(c):
    c = LINK.sub(lambda m: m.group(1), c)
    c = re.sub(r"'''?", "", c)
    c = re.sub(r"<[^>]+>", "", c)
    c = re.sub(r"style\s*=\s*\"[^\"]*\"\s*\|", "", c)  # cell attribute prefix
    return c.strip()

def iter_table_rows(table_text):
    """Yield rows of a {| ... |} wikitext table as lists of cleaned cell strings."""
    for chunk in re.split(r"\n\|-.*", table_text):
        cells = []
        for line in chunk.splitlines():
            line = line.strip()
            if line.startswith("{|") or line.startswith("|}"):
                continue
            if line.startswith("!"):
                cells += [strip_cell(c) for c in line.lstrip("!").split("!!")]
            elif line.startswith("|"):
                cells += [strip_cell(c) for c in line.lstrip("|").split("||")]
        if cells:
            yield cells

def find_tables(wt):
    return re.findall(r"\{\|.*?\|\}", wt, flags=re.S)

def parse_statistics(wt):
    """-> (race_rows {race_name: {stat: value}}, race_notes {race_name: note},
           class_mods {abbr: {stat: value}})"""
    race_rows, race_notes, class_mods = {}, {}, {}
    for table in find_tables(wt):
        rows = list(iter_table_rows(table))
        if not rows:
            continue
        header = [h.upper() for h in rows[0]]
        if header[0] == "RACE":
            col_of = {st: header.index(st) for st in STATS}   # STR..CHA present
            total_col = header.index("TOTAL") if "TOTAL" in header else None
            for row in rows[1:]:
                first = row[0]
                m = re.match(r"^([^()]+?)\s*(\((.*)\))?\s*$", first)
                name = RACE_ALIASES.get(m.group(1).strip(), m.group(1).strip())
                if m.group(3):
                    race_notes[name] = m.group(3).strip()
                vals = {st: int(row[col_of[st]]) for st in STATS}
                if total_col is not None and row[total_col].strip():
                    tot = int(row[total_col])
                    if tot != sum(vals.values()):
                        print("  WARNING: %s stat sum %d != wiki Total %d"
                              % (name, sum(vals.values()), tot))
                race_rows[name] = vals
        elif header[0] == "CLASS":
            col_of = {st: header.index(st) for st in STATS}
            for row in rows[1:]:
                cname = row[0].strip()
                abbr = CLASS_NAME2ABBR.get(cname)
                if abbr is None:
                    print("  WARNING: unknown class name %r in class table" % cname)
                    continue
                class_mods[abbr] = {st: int(row[col_of[st]] or 0) if
                                    row[col_of[st]].strip() else 0 for st in STATS}
    return race_rows, race_notes, class_mods

# ----------------------------------------------------------------------- main
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=DB)
    ap.add_argument("--refetch", action="store_true",
                    help="delete the cached Statistics wikitext first")
    args = ap.parse_args()

    if args.refetch:
        for f in ("Statistics.wikitext", "Statistics.meta.json"):
            p = os.path.join(FIXDIR, f)
            if os.path.exists(p):
                os.remove(p)

    print("== fetch ==")
    wt, revid = fetch_wikitext("Statistics")
    if wt is None:
        raise SystemExit("Statistics page missing on the wiki")

    race_rows, race_notes, class_mods = parse_statistics(wt)
    # If the Statistics page ever loses the race table, fall back to Races page
    if not race_rows:
        print("  race table not on Statistics; trying 'Races' page")
        wt2, _ = fetch_wikitext("Races")
        if wt2:
            r2, n2, _ = parse_statistics(wt2)
            race_rows, race_notes = r2, n2

    if len(race_rows) != 15:
        print("  WARNING: expected 15 races, parsed %d: %s"
              % (len(race_rows), sorted(race_rows)))
    if len(class_mods) != 16:
        print("  WARNING: expected 16 classes, parsed %d: %s"
              % (len(class_mods), sorted(class_mods)))

    con = sqlite3.connect(args.db)
    cur = con.cursor()
    cur.execute("""CREATE TABLE IF NOT EXISTS race(
        id INTEGER PRIMARY KEY, name TEXT UNIQUE)""")
    cur.execute("""CREATE TABLE IF NOT EXISTS race_base_stats(
        race_id INTEGER, stat TEXT, value INTEGER, PRIMARY KEY(race_id, stat))""")
    cur.execute("""CREATE TABLE IF NOT EXISTS class_stat_mod(
        class_id INTEGER PRIMARY KEY,
        str INTEGER DEFAULT 0, sta INTEGER DEFAULT 0, agi INTEGER DEFAULT 0,
        dex INTEGER DEFAULT 0, wis INTEGER DEFAULT 0, intel INTEGER DEFAULT 0,
        cha INTEGER DEFAULT 0)""")

    for rid, name in RACE_SEED:  # plan Group-12 constants
        cur.execute("INSERT OR REPLACE INTO race(id, name) VALUES(?,?)", (rid, name))
    race_id = {name: rid for rid, name in RACE_SEED}

    n_stats = 0
    for name, vals in sorted(race_rows.items()):
        rid = race_id.get(name)
        if rid is None:
            print("  WARNING: race %r not in the plan's 15-race seed; skipped" % name)
            continue
        for st in STATS:
            cur.execute("""INSERT OR REPLACE INTO race_base_stats(race_id, stat, value)
                           VALUES(?,?,?)""", (rid, st, vals[st]))
            n_stats += 1

    for abbr, vals in sorted(class_mods.items()):
        cur.execute("""INSERT OR REPLACE INTO class_stat_mod
                       (class_id, str, sta, agi, dex, wis, intel, cha)
                       VALUES(?,?,?,?,?,?,?,?)""",
                    (CLASS_ID[abbr], vals["STR"], vals["STA"], vals["AGI"],
                     vals["DEX"], vals["WIS"], vals["INT"], vals["CHA"]))
    con.commit()

    # ------------------------------------------------------------------ verify
    print("\n== verify ==")
    bad = [name for (name,) in cur.execute(
        """SELECT r.name FROM race r LEFT JOIN race_base_stats b ON b.race_id=r.id
           GROUP BY r.id HAVING COUNT(b.stat) <> 7""")]
    print("race_base_stats rows written: %d (%d races x 7 stats)"
          % (n_stats, len(race_rows)))
    print("races missing stats: %s" % (bad if bad else "none"))
    print("class_stat_mod rows: %d" % cur.execute(
        "SELECT COUNT(*) FROM class_stat_mod").fetchone()[0])
    if revid:
        print("source: Statistics revid %s" % revid)
    for name, note in race_notes.items():
        print("wiki note (printed, NOT modeled): %s -- %r" % (name, note))

    print("\n-- race x stat matrix --")
    print("%-10s %s  TOTAL" % ("race", "  ".join("%4s" % s for s in STATS)))
    for rid, name in RACE_SEED:
        vals = dict(cur.execute(
            "SELECT stat, value FROM race_base_stats WHERE race_id=?", (rid,)))
        if vals:
            print("%-10s %s  %5d" % (name,
                  "  ".join("%4d" % vals[s] for s in STATS), sum(vals.values())))
        else:
            print("%-10s (no stats)" % name)

    print("\n-- class additive mod matrix --")
    print("%-14s %s  TOTAL" % ("class", "  ".join("%4s" % s for s in STATS)))
    for row in cur.execute("""SELECT c.abbr, m.str, m.sta, m.agi, m.dex, m.wis,
                              m.intel, m.cha FROM class_stat_mod m
                              JOIN class c ON c.id=m.class_id ORDER BY m.class_id"""):
        print("%-14s %s  %5d" % (row[0],
              "  ".join("%4d" % v for v in row[1:]), sum(row[1:])))

    con.close()
    print("\nrequests used: %d / %d" % (_request_count, MAX_REQUESTS))
    print("done.")

if __name__ == "__main__":
    main()
