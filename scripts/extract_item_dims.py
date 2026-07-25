#!/usr/bin/env python3
"""Extract weight + size (and any other display-only dims) from items.raw_statsblock
into REAL columns, so they survive make_dist_db.py (which nulls the raw column).

Idempotent — run any time after a sync:  python scripts/extract_item_dims.py
The statsblock line looks like:  WT: 1.5  Size: SMALL<br>
"""
import re
import sqlite3
from pathlib import Path

DB = Path(__file__).resolve().parent.parent / "db" / "eql.db"

WT_RE = re.compile(r"WT:\s*([\d.]+)", re.I)
SIZE_RE = re.compile(r"Size:\s*([A-Z]+)", re.I)


def main() -> None:
    con = sqlite3.connect(DB)
    cols = {r[1] for r in con.execute("PRAGMA table_info(items)")}
    if "weight" not in cols:
        con.execute("ALTER TABLE items ADD COLUMN weight REAL")
    if "size" not in cols:
        con.execute("ALTER TABLE items ADD COLUMN size TEXT")

    n_wt = n_sz = n_raw = 0
    for pageid, raw in con.execute(
        "SELECT pageid, raw_statsblock FROM items "
        "WHERE raw_statsblock IS NOT NULL AND raw_statsblock != ''"
    ).fetchall():
        n_raw += 1
        wt = WT_RE.search(raw)
        sz = SIZE_RE.search(raw)
        weight = float(wt.group(1)) if wt else None
        size = sz.group(1).upper() if sz else None
        if weight is not None or size is not None:
            con.execute(
                "UPDATE items SET weight=COALESCE(?, weight), size=COALESCE(?, size) "
                "WHERE pageid=?",
                (weight, size, pageid),
            )
            n_wt += weight is not None
            n_sz += size is not None
    con.commit()
    have_wt = con.execute("SELECT COUNT(*) FROM items WHERE weight IS NOT NULL").fetchone()[0]
    have_sz = con.execute("SELECT COUNT(*) FROM items WHERE size IS NOT NULL").fetchone()[0]
    print(f"scanned {n_raw} statsblocks: weight set {n_wt} (total {have_wt}), "
          f"size set {n_sz} (total {have_sz})")
    con.close()


if __name__ == "__main__":
    main()
