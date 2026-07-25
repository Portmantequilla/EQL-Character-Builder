"""Tag the class epic quest weapons in db/eql.db (items.is_epic).

The optimizer skips epics unless the user enables "Allow epic gear" — they are
long quest chains, not drops. The name list lives in overrides/seeds/epic_items.json.

Idempotent: resets the flag and re-marks from the list on every run.
Run after eql_wiki_sync.py and before make_dist_db.py.
"""
import json
import os
import sqlite3
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
DB = os.path.join(ROOT, "db", "eql.db")
SEED = os.path.join(ROOT, "overrides", "seeds", "epic_items.json")


def main():
    if not os.path.exists(DB):
        sys.exit("missing %s -- run scripts/eql_wiki_sync.py first" % DB)
    names = json.load(open(SEED, encoding="utf-8"))["names"]

    con = sqlite3.connect(DB)
    cur = con.cursor()
    cur.execute("PRAGMA table_info(items)")
    if not any(r[1] == "is_epic" for r in cur.fetchall()):
        cur.execute("ALTER TABLE items ADD COLUMN is_epic INTEGER NOT NULL DEFAULT 0")
        print("  added column items.is_epic")

    cur.execute("UPDATE items SET is_epic = 0")
    ph = ",".join("?" * len(names))
    cur.execute("UPDATE items SET is_epic = 1 WHERE name IN (%s)" % ph, names)
    con.commit()

    marked = cur.execute("SELECT COUNT(*) FROM items WHERE is_epic = 1").fetchone()[0]
    missing = [n for n in names
               if not cur.execute("SELECT 1 FROM items WHERE name = ?", (n,)).fetchone()]
    con.close()
    print("  marked %d epic items (%d names in list)" % (marked, len(names)))
    if missing:
        print("  NOTE: not found in the mirror (fine if the game lacks them): %s"
              % ", ".join(missing))


if __name__ == "__main__":
    main()
