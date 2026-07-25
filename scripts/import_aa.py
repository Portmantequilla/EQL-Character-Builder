#!/usr/bin/env python3
"""import_aa.py — pull the wiki's Alternate Advancement tables into db/eql.db.

The AA page (one page, 19 wikitables) is the whole system:
  General   — every class
  Archetype — depends on your class combo (the page does not say which; see note)
  Class     — one table per class (16)
  Special   — special tab

Each row: Name || Ranks || Cost (per-rank, "3/6/9", may contain "?") || Description
(the description carries "Requirements: level N.").

Run: python scripts/import_aa.py          (fetches; caches to raw/fixtures/aa_page.wikitext)
     python scripts/import_aa.py --offline  (re-parse the cache, no network)
"""
import argparse
import json
import os
import re
import sqlite3
import sys

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CACHE = os.path.join(BASE, "raw", "fixtures", "aa_page.wikitext")
DB = os.environ.get("EQL_DB", os.path.join(BASE, "db", "eql.db"))
API = "https://eqlwiki.com/api.php"
UA = "EQL-Wiki-Sync/1.0 (personal theorycraft tool)"

CLASS_BY_NAME = {
    "Warrior": "WAR", "Cleric": "CLR", "Paladin": "PAL", "Ranger": "RNG",
    "Shadow Knight": "SHD", "Druid": "DRU", "Monk": "MNK", "Bard": "BRD",
    "Rogue": "ROG", "Shaman": "SHM", "Necromancer": "NEC", "Wizard": "WIZ",
    "Magician": "MAG", "Enchanter": "ENC", "Beastlord": "BST", "Berserker": "BER",
}

DDL = """
CREATE TABLE IF NOT EXISTS aa(
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  category TEXT NOT NULL CHECK (category IN ('GENERAL','ARCHETYPE','CLASS','SPECIAL')),
  class_abbr TEXT,                 -- CLASS rows only; NULL elsewhere
  max_rank INTEGER NOT NULL,
  cost_json TEXT NOT NULL,         -- per-rank costs; null entries = wiki wrote "?"
  total_cost INTEGER,              -- sum of known costs (NULL entries treated as 0)
  cost_complete INTEGER NOT NULL,  -- 0 = the wiki has "?" in the cost list
  required_level INTEGER,
  description TEXT,
  source_revision INTEGER
  -- deliberately NO unique(name,...): the wiki really does list two different AAs
  -- with the same name (Cleric "Divine Aura" x2, different cost + effect). Collapsing
  -- them would silently drop one, so keep both and report the collision.
);
CREATE INDEX IF NOT EXISTS idx_aa_cat ON aa(category, class_abbr);
CREATE INDEX IF NOT EXISTS idx_aa_name ON aa(name);
"""


def fetch():
    import requests
    s = requests.Session()
    s.headers["User-Agent"] = UA
    r = s.get(API, params={"action": "query", "titles": "Alternate Advancement",
                           "prop": "revisions", "rvprop": "content|ids",
                           "rvslots": "main", "format": "json"}, timeout=60)
    pg = list(r.json()["query"]["pages"].values())[0]
    rev = pg["revisions"][0]
    os.makedirs(os.path.dirname(CACHE), exist_ok=True)
    with open(CACHE, "w", encoding="utf-8") as fh:
        fh.write(rev["slots"]["main"]["*"])
    return rev["slots"]["main"]["*"], rev["revid"]


def strip_wiki(s):
    s = re.sub(r"\[\[(?:[^\]|]*\|)?([^\]|]*)\]\]", r"\1", s)   # links
    s = re.sub(r"'''?", "", s)                                  # bold/italic
    s = re.sub(r"<[^>]+>", "", s)                               # html
    return re.sub(r"\s+", " ", s).strip()


def parse_costs(cell):
    """'3/6/9' -> [3,6,9]; '2/4/6/?' -> [2,4,6,None]; '9' -> [9]."""
    out = []
    for part in strip_wiki(cell).split("/"):
        p = part.strip()
        m = re.search(r"\d+", p)
        out.append(int(m.group()) if m else None)
    return out


def parse(text, revid):
    rows = []
    # walk the page section by section so each table knows its category/class
    sections = re.split(r"^(==+ *[^=]+? *==+)\s*$", text, flags=re.M)
    category, class_abbr = None, None
    for chunk in sections:
        h = re.match(r"^(==+) *([^=]+?) *==+$", chunk.strip())
        if h:
            title = h.group(2).strip()
            if title == "General AAs":
                category, class_abbr = "GENERAL", None
            elif title == "Archetype AAs":
                category, class_abbr = "ARCHETYPE", None
            elif title == "Class AAs":
                category, class_abbr = "CLASS", None
            elif title == "Special AAs":
                category, class_abbr = "SPECIAL", None
            elif title.endswith("Class AAs"):
                cname = title[:-len(" Class AAs")].strip()
                class_abbr = CLASS_BY_NAME.get(cname)
                category = "CLASS"
                if not class_abbr:
                    print(f"  ! unknown class heading: {title}")
            continue
        if not category or "{|" not in chunk:
            continue
        for table in re.findall(r"\{\|.*?\|\}", chunk, re.S):
            for line in table.split("\n|-")[1:]:
                cells = [c.strip() for c in re.split(r"\|\|", line.lstrip("|\n "))]
                if len(cells) < 3:
                    continue
                name = strip_wiki(cells[0])
                if not name or name.lower() == "name":
                    continue
                mr = re.search(r"\d+", strip_wiki(cells[1]))
                if not mr:
                    continue
                max_rank = int(mr.group())
                costs = parse_costs(cells[2])
                desc = strip_wiki(cells[3]) if len(cells) > 3 else ""
                lv = re.search(r"Requirements?:\s*level\s*(\d+)", desc, re.I)
                rows.append(dict(
                    name=name, category=category, class_abbr=class_abbr,
                    max_rank=max_rank, costs=costs,
                    total=sum(c for c in costs if c),
                    complete=int(all(c is not None for c in costs) and len(costs) == max_rank),
                    level=int(lv.group(1)) if lv else None,
                    desc=desc, revid=revid,
                ))
    return rows


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--offline", action="store_true")
    a = ap.parse_args()

    if a.offline:
        if not os.path.exists(CACHE):
            sys.exit("no cache — run once without --offline")
        text, revid = open(CACHE, encoding="utf-8").read(), None
    else:
        text, revid = fetch()

    rows = parse(text, revid)
    con = sqlite3.connect(DB)
    con.execute("DROP TABLE IF EXISTS aa")  # schema changed; rebuild cleanly
    con.executescript(DDL)
    for r in rows:
        con.execute(
            "INSERT INTO aa(name,category,class_abbr,max_rank,cost_json,"
            "total_cost,cost_complete,required_level,description,source_revision) "
            "VALUES(?,?,?,?,?,?,?,?,?,?)",
            (r["name"], r["category"], r["class_abbr"], r["max_rank"],
             json.dumps(r["costs"]), r["total"], r["complete"], r["level"],
             r["desc"], r["revid"]))
    con.commit()

    # the wiki has same-name AAs (e.g. Cleric "Divine Aura" twice) — surface them
    dupes = con.execute(
        "SELECT name, category, COALESCE(class_abbr,'-'), COUNT(*) FROM aa "
        "GROUP BY name, category, class_abbr HAVING COUNT(*) > 1").fetchall()
    if dupes:
        print("  name collisions kept as separate AAs (wiki lists them twice):")
        for n, c, cl, k in dupes:
            print(f"     {n} [{c}/{cl}] x{k}")

    print(f"AAs imported: {len(rows)}")
    for cat, n in con.execute("SELECT category, COUNT(*) FROM aa GROUP BY category"):
        print(f"   {cat:10} {n}")
    inc = con.execute("SELECT COUNT(*) FROM aa WHERE cost_complete=0").fetchone()[0]
    print(f"   (rows whose wiki cost list has '?': {inc} — cost shown as partial)")
    mnem = con.execute(
        "SELECT name, max_rank, cost_json, required_level FROM aa "
        "WHERE name='Mnemonic Retention'").fetchone()
    print("   sanity, Mnemonic Retention:", mnem)
    if len(rows) < 100:
        sys.exit("FAIL: suspiciously few AAs parsed")


if __name__ == "__main__":
    main()
