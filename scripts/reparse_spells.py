"""Rebuild the spell tables OFFLINE from the cached raw/ dumps (no network, no re-download).

Use after changing the parser or the spell-domain schema in eql_wiki_sync.py:
    python scripts/reparse_spells.py

Drops + recreates spell/pet tables (so schema changes take effect), re-parses
raw/spells.json + npc_only_spells.json + summoned_pet.json + beastlord_pet.json,
re-applies the NPC-only category flag, resolves name links, prints the quality report.
Items/mobs tables are untouched.
"""
import os, json, sqlite3, importlib.util

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
spec = importlib.util.spec_from_file_location(
    "eqlsync", os.path.join(BASE, "scripts", "eql_wiki_sync.py"))
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

_pre = sqlite3.connect(mod.DB)
for t in ["spell", "pet_stat_block"] + mod.SPELL_CHILDREN:
    _pre.execute(f"DROP TABLE IF EXISTS {t}")
_pre.execute("DROP VIEW IF EXISTS v_spell_class")
_pre.commit(); _pre.close()

con = mod.db()
stats, npc_ids = {}, []
for fname in ("spells.json", "npc_only_spells.json", "summoned_pet.json", "beastlord_pet.json"):
    path = os.path.join(BASE, "raw", fname)
    if not os.path.exists(path):
        print(f"SKIP {fname} (not cached - run sync-spells once first)"); continue
    rows = json.load(open(path, encoding="utf-8"))
    pages = [(int(r["pageid"]), r["title"], r["wikitext"], r.get("revid")) for r in rows]
    mod.load_pages(con, pages, stats)
    if fname == "npc_only_spells.json":
        npc_ids = [int(r["pageid"]) for r in rows]
    print(f"{fname}: {len(pages)} pages")
con.executemany("UPDATE spell SET is_npc_only=1 WHERE id=?", [(i,) for i in npc_ids])
con.commit()
fixed = mod.finalize_spells(con)

q = lambda sql: con.execute(sql).fetchone()[0]
effects = q("SELECT COUNT(*) FROM spell_effect WHERE is_stacking_rule=0")
unparsed = q("SELECT COUNT(*) FROM spell_effect WHERE opcode='UNPARSED'")
print("\n=== re-parse report ===")
print(f"spells={q('SELECT COUNT(*) FROM spell')}"
      f"  class_rows={q('SELECT COUNT(*) FROM spell_class_level')}"
      f"  effects={effects}  stacking={q('SELECT COUNT(*) FROM spell_stacking_rule')}"
      f"  resolved_links={fixed}")
print(f"songs={q('SELECT COUNT(*) FROM bard_song_rule')}"
      f"  pet_summons={q('SELECT COUNT(*) FROM spell_pet_summon')}"
      f"  warders={q('SELECT COUNT(*) FROM pet_stat_block')}"
      f"  npc_only={q('SELECT COUNT(*) FROM spell WHERE is_npc_only=1')}")
pct = 100.0 * unparsed / effects if effects else 0.0
print(f"UNPARSED={unparsed} ({pct:.2f}%)"
      f"  UNKNOWN_STAT={q(chr(39).join(['SELECT COUNT(*) FROM spell_effect WHERE opcode=', 'UNKNOWN_STAT', '']))}")
if unparsed:
    for name, raw in con.execute("SELECT s.name, e.raw_text FROM spell_effect e "
                                 "JOIN spell s ON s.id=e.spell_id "
                                 "WHERE e.opcode='UNPARSED' LIMIT 25"):
        print(f"  UNPARSED  {name}: {raw!r}")
print("GATE:", "PASS (<2%)" if pct < 2.0 else "FAIL (>=2%)")
print("Remember: python scripts/eql_wiki_sync.py export  (to refresh exports/*.json)")
