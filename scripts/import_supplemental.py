"""Load overrides/seeds/supplemental_items.json into db/eql.db.

These rows are flagged `canonical = 0` so the app hides them from the pickers
unless deliberately revealed. They are intentional and must NOT be corrected
against live game data -- see CONTRIBUTING.md.

Idempotent: every run clears the reserved id range first, then re-inserts.
Run after eql_wiki_sync.py and before make_dist_db.py.
"""
import json
import os
import sqlite3
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
DB = os.path.join(ROOT, "db", "eql.db")
SEED = os.path.join(ROOT, "overrides", "seeds", "supplemental_items.json")

# every class abbr, so the entries are wearable regardless of the pet's own pool
ALL_CLASSES = ["WAR", "CLR", "PAL", "RNG", "SHD", "DRU", "MNK", "BRD",
               "ROG", "SHM", "NEC", "WIZ", "MAG", "ENC", "BST", "BER"]


def column_exists(cur, table, col):
    cur.execute("PRAGMA table_info(%s)" % table)
    return any(r[1] == col for r in cur.fetchall())


def ensure_column(cur, table, col, decl):
    if not column_exists(cur, table, col):
        cur.execute("ALTER TABLE %s ADD COLUMN %s %s" % (table, col, decl))
        print("  added column %s.%s" % (table, col))


def table_exists(cur, name):
    cur.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (name,))
    return cur.fetchone() is not None


def main():
    if not os.path.exists(DB):
        sys.exit("missing %s -- run scripts/eql_wiki_sync.py first" % DB)
    if not os.path.exists(SEED):
        sys.exit("missing %s" % SEED)

    data = json.load(open(SEED, encoding="utf-8"))
    lo, hi = data.get("id_range", [777000, 777999])
    con = sqlite3.connect(DB)
    cur = con.cursor()

    # the flag itself: default 1 so every existing row stays canonical
    ensure_column(cur, "items", "canonical", "INTEGER NOT NULL DEFAULT 1")
    ensure_column(cur, "spell", "canonical", "INTEGER NOT NULL DEFAULT 1")

    # ---- clear the reserved range (idempotent re-run) ----
    for tbl, key in [("item_stats", "pageid"), ("item_classes", "pageid"),
                     ("item_races", "pageid"), ("item_slots", "pageid"),
                     ("items", "pageid"), ("spell_class_level", "spell_id"),
                     ("spell", "id")]:
        if table_exists(cur, tbl):
            cur.execute("DELETE FROM %s WHERE %s BETWEEN ? AND ?" % (tbl, key), (lo, hi))

    # ---- items ----
    n_items = 0
    for it in data.get("items", []):
        cur.execute(
            "INSERT INTO items(pageid, name, slot, ac, dmg, atk_delay, weapon_skill, "
            "haste_pct, worn_effect, flags, era, canonical) "
            "VALUES(?,?,?,?,?,?,?,?,?,?,?,0)",
            (it["pageid"], it["name"], it.get("slot"), it.get("ac"), it.get("dmg"),
             it.get("atk_delay"), it.get("weapon_skill"), it.get("haste_pct"),
             it.get("worn_effect"), it.get("flags"), it.get("era")),
        )
        for stat, val in (it.get("stats") or {}).items():
            cur.execute("INSERT INTO item_stats(pageid, stat, value) VALUES(?,?,?)",
                        (it["pageid"], stat, val))
        for cl in ALL_CLASSES:
            cur.execute("INSERT INTO item_classes(pageid, class) VALUES(?,?)",
                        (it["pageid"], cl))
        cur.execute("INSERT INTO item_races(pageid, race) VALUES(?,?)", (it["pageid"], "ALL"))
        if table_exists(cur, "item_slots"):
            for sl in it.get("slots", []):
                cur.execute("INSERT INTO item_slots(pageid, slot) VALUES(?,?)",
                            (it["pageid"], sl))
        n_items += 1

    # ---- spells ----
    # CHECK constraints guard role/resist_type/era_source/template_name, so those
    # columns are left NULL rather than guessed at.
    n_spells = 0
    cur.execute("SELECT id FROM class WHERE abbr='MAG'")
    row = cur.fetchone()
    mag_id = row[0] if row else None
    for sp in data.get("spells", []):
        cur.execute(
            "INSERT INTO spell(id, name, name_canonical, page_title, mana, "
            "target_type_raw, description, is_beneficial, is_npc_only, canonical) "
            "VALUES(?,?,?,?,?,?,?,?,0,0)",
            (sp["id"], sp["name"], sp["name"].lower(), sp["name"], sp.get("mana"),
             sp.get("target"), sp.get("desc"), 1),
        )
        if mag_id is not None and table_exists(cur, "spell_class_level"):
            cur.execute(
                "INSERT INTO spell_class_level(spell_id, class_id, required_class_level, "
                "is_autogranted) VALUES(?,?,?,0)",
                (sp["id"], mag_id, sp.get("level", 1)),
            )
        n_spells += 1

    con.commit()
    cur.execute("SELECT COUNT(*) FROM items WHERE canonical = 0")
    ci = cur.fetchone()[0]
    cur.execute("SELECT COUNT(*) FROM spell WHERE canonical = 0")
    cs = cur.fetchone()[0]
    con.close()
    print("  inserted %d items, %d spells (non-canonical totals: %d / %d)"
          % (n_items, n_spells, ci, cs))
    if ci != n_items or cs != n_spells:
        sys.exit("FAIL: non-canonical row counts do not match the seed file")


if __name__ == "__main__":
    main()
