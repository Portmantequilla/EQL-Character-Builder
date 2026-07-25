#!/usr/bin/env python3
"""fetch_icons.py - download the distinct item icons from eqlwiki.com into
app/public/icons/item_<id>.png (served by vite in dev, bundled by tauri build).

MediaWiki stores File:Item_<id>.png at /images/<m>/<mn>/Item_<id>.png where m/mn are
the first chars of md5(filename) - computable locally, so this is ONE GET per icon.
Polite: 0.3s pacing, descriptive UA, skip-existing (re-runs only fetch new icons).

Run: python scripts/fetch_icons.py
"""
import hashlib
import os
import sqlite3
import sys
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
    ids = [r[0] for r in con.execute(
        "SELECT DISTINCT icon_id FROM items WHERE icon_id IS NOT NULL ORDER BY icon_id")]
    s = requests.Session()
    s.headers["User-Agent"] = UA
    ok = skipped = missing = 0
    misses = []
    for i, icon in enumerate(ids):
        path = os.path.join(OUT, f"item_{icon}.png")
        if os.path.exists(path) and os.path.getsize(path) > 0:
            skipped += 1
            continue
        url = hashed_url(f"Item_{icon}.png")
        try:
            r = s.get(url, timeout=30)
        except requests.RequestException as e:
            print(f"  ERROR {icon}: {e}", flush=True)
            misses.append(icon)
            missing += 1
            time.sleep(1.0)
            continue
        if r.status_code == 200 and r.content[:8].startswith(b"\x89PNG"):
            with open(path, "wb") as fh:
                fh.write(r.content)
            ok += 1
        else:
            misses.append(icon)
            missing += 1
        if (i + 1) % 50 == 0:
            print(f"  {i + 1}/{len(ids)} (ok={ok} skip={skipped} miss={missing})", flush=True)
        time.sleep(0.3)
    print(f"\nDone: {ok} downloaded, {skipped} already present, {missing} missing")
    if misses:
        print("missing icon ids:", misses[:40], "..." if len(misses) > 40 else "")
        # count how many items are affected by the missing icons
        ph = ",".join("?" * len(misses))
        n = con.execute(
            f"SELECT COUNT(*) FROM items WHERE icon_id IN ({ph})", misses).fetchone()[0]
        print(f"({n} items reference missing icons - UI must fall back to a blank well)")
    sys.exit(0)


if __name__ == "__main__":
    main()
