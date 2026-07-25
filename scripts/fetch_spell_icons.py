#!/usr/bin/env python3
"""fetch_spell_icons.py - download the distinct spell icons (spellicon_<X>.png) from
eqlwiki.com into app/public/icons/spellicon_<X>.png for the Spellbook tab.
Same polite pattern as fetch_icons.py; MediaWiki uppercases the first filename letter,
so try Spellicon_<X>.png first, then spellicon_<X>.png."""
import hashlib
import os
import sqlite3
import time

import requests

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(BASE, "app", "public", "icons")
UA = "EQL-Wiki-Sync/1.0 (personal theorycraft tool)"
SITE = "https://eqlwiki.com"


def hashed_url(filename: str) -> str:
    h = hashlib.md5(filename.replace(" ", "_").encode("utf-8")).hexdigest()
    return f"{SITE}/images/{h[0]}/{h[:2]}/{filename}"


def main():
    os.makedirs(OUT, exist_ok=True)
    con = sqlite3.connect(os.path.join(BASE, "db", "eql.db"))
    icons = [r[0] for r in con.execute(
        "SELECT DISTINCT icon FROM spell WHERE icon IS NOT NULL ORDER BY icon")]
    s = requests.Session()
    s.headers["User-Agent"] = UA
    ok = skipped = missing = 0
    misses = []
    for icon in icons:
        path = os.path.join(OUT, f"spellicon_{icon}.png")
        if os.path.exists(path) and os.path.getsize(path) > 0:
            skipped += 1
            continue
        got = False
        for fname in (f"Spellicon_{icon}.png", f"spellicon_{icon}.png"):
            try:
                r = s.get(hashed_url(fname), timeout=30)
            except requests.RequestException:
                continue
            if r.status_code == 200 and r.content[:8].startswith(b"\x89PNG"):
                with open(path, "wb") as fh:
                    fh.write(r.content)
                ok += 1
                got = True
                break
            time.sleep(0.2)
        if not got:
            missing += 1
            misses.append(icon)
        time.sleep(0.3)
    print(f"Done: {ok} downloaded, {skipped} present, {missing} missing")
    if misses:
        print("missing:", misses)


if __name__ == "__main__":
    main()
