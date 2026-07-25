"""Golden tests for the spell parser: parse the cached fixture pages in raw/fixtures/
into a THROWAWAY temp DB and assert known-good facts. No network, never touches db/eql.db.

Run:  python scripts/test_spell_parse.py
"""
import os, re, sys, glob, tempfile, importlib.util

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DBPATH = os.path.join(tempfile.gettempdir(), "eql_golden_test.db")
if os.path.exists(DBPATH):
    os.remove(DBPATH)
os.environ["EQL_DB"] = DBPATH

spec = importlib.util.spec_from_file_location(
    "eqlsync", os.path.join(BASE, "scripts", "eql_wiki_sync.py"))
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)

pages, seen, fake_id = [], set(), 900000
for folder in ("probe-spells", "probe-pets"):
    for path in sorted(glob.glob(os.path.join(BASE, "raw", "fixtures", folder, "*.wikitext"))):
        base = os.path.basename(path)
        if base.startswith("Template_") or base == "Buff_Lines.wikitext" or base in seen:
            continue
        seen.add(base)
        text = open(path, encoding="utf-8").read()
        m = re.match(r"<!-- title: (.*?) \| pageid: (\d+) \| revid: (\d+)", text)
        if m:
            title, pid, revid = m.group(1), int(m.group(2)), int(m.group(3))
        else:
            fake_id += 1
            title, pid, revid = base[:-9].replace("_", " "), fake_id, None
        pages.append((pid, title, text, revid))

con = mod.db()
stats = {}
ni, nm = mod.load_pages(con, pages, stats)
fixed = mod.finalize_spells(con)
print(f"pages={len(pages)} spells={stats.get('spells',0)} petpages={stats.get('petpages',0)} "
      f"resolved={fixed}")

fails = []
def check(desc, sql, expect):
    got = con.execute(sql).fetchall()
    ok = expect(got)
    print(("PASS  " if ok else "FAIL  ") + desc + ("" if ok else f"  -> {got}"))
    if not ok:
        fails.append(desc)

check("Aegolism stacking slot row",
      "SELECT rule_type, affected_effect_slot, affected_effect_opcode, comparison_operator,"
      " comparison_value, source_type, verified FROM spell_stacking_rule"
      " WHERE spell_id=49969 AND source_type='WIKI_SLOT_ROW'",
      lambda r: r == [("BLOCK_IF_PRESENT", 3, "MAX_HP", "<", 1100.0, "WIKI_SLOT_ROW", 1)])
check("Aegolism prose OVERWRITE_ALWAYS x3",
      "SELECT COUNT(*) FROM spell_stacking_rule WHERE spell_id=49969"
      " AND rule_type='OVERWRITE_ALWAYS' AND source_type='WIKI_PROSE'",
      lambda r: r[0][0] == 3)
check("Aegolism effects: MAX_HP 1100 + HP_WHEN_CAST + AC 54",
      "SELECT opcode, base_amount FROM spell_effect WHERE spell_id=49969"
      " AND is_stacking_rule=0 ORDER BY slot_number",
      lambda r: r == [("MAX_HP", 1100.0), ("HP_WHEN_CAST", 1100.0), ("AC", 54.0)])
check("SoW class rows",
      "SELECT c.abbr, l.required_class_level, l.is_autogranted FROM spell_class_level l"
      " JOIN class c ON c.id=l.class_id WHERE l.spell_id=49998 ORDER BY c.abbr",
      lambda r: r == [("BST", 24, 0), ("DRU", 10, 1), ("RNG", 28, 0), ("SHM", 9, 0)])
check("SoW effect MOVE_SPEED 30 pct",
      "SELECT opcode, base_amount, is_percent FROM spell_effect WHERE spell_id=49998"
      " AND is_stacking_rule=0",
      lambda r: r == [("MOVE_SPEED", 30.0, 1)])
check("SoW era Classic via TAG",
      "SELECT era, era_source FROM spell WHERE id=49998",
      lambda r: r == [("Classic", "TAG")])
check("SoW items_with_effect count",
      "SELECT COUNT(*) FROM spell_item_source WHERE spell_id=49998",
      lambda r: r[0][0] == 8)
check("Bone Walk pet summon block",
      "SELECT pet_classes, base_pet_level, base_level_source, base_pet_level_status,"
      " pet_hp_numeric, pet_max_hit FROM spell_pet_summon WHERE spell_id=50150",
      lambda r: r == [("WAR/SHD", 9, "OTHER_BLOCK", "WIKI_CONFIRMED", 400, 16)])
check("Bone Walk classes",
      "SELECT c.abbr, l.required_class_level, l.is_autogranted FROM spell_class_level l"
      " JOIN class c ON c.id=l.class_id WHERE l.spell_id=50150 ORDER BY c.abbr",
      lambda r: r == [("NEC", 8, 1), ("SHD", 14, 0)])
check("Bone Walk role PET_SUMMON + vendor rows",
      "SELECT role, (SELECT COUNT(*) FROM spell_source WHERE spell_id=50150"
      " AND source_type='VENDOR') FROM spell WHERE id=50150",
      lambda r: len(r) == 1 and r[0][0] == "PET_SUMMON" and r[0][1] == 12)
check("Anthem song flags",
      "SELECT s.is_song, t.target_base, b.instrument_type, b.instrument_scaling_allowed"
      " FROM spell s JOIN spell_target_rule t ON t.spell_id=s.id"
      " JOIN bard_song_rule b ON b.spell_id=s.id WHERE s.id=45502",
      lambda r: r == [(1, "GROUP", "SINGING", "YES")])
check("Anthem STR scaling",
      "SELECT opcode, base_amount, max_amount, min_caster_level, max_caster_level,"
      " caster_level_scaling FROM spell_effect WHERE spell_id=45502 AND opcode='STR'",
      lambda r: r == [("STR", 10.0, 35.0, 10, 60, "LINEAR_ASSUMED")])
check("Anthem duration BARD_PULSE 3 ticks",
      "SELECT duration_class, tick_count, maintenance_type FROM spell_duration_rule"
      " WHERE spell_id=45502",
      lambda r: r == [("BARD_PULSE", 3, "BARD_SONG")])
check("Aria of Eagles is a song despite dagger footnote in skill",
      "SELECT s.is_song, b.instrument_type FROM spell s"
      " JOIN bard_song_rule b ON b.spell_id=s.id WHERE s.id=50679",
      lambda r: r == [(1, "WIND")])
check("Burnout PET target + role",
      "SELECT s.role, t.target_base, t.pet_targetable FROM spell s"
      " JOIN spell_target_rule t ON t.spell_id=s.id WHERE s.id=50366",
      lambda r: r == [("PET_BUFF", "PET", 1)])
check("Burnout sparse slots 3,4",
      "SELECT slot_number FROM spell_effect WHERE spell_id=50366 ORDER BY slot_number",
      lambda r: [x[0] for x in r] == [3, 4])
check("Splurt ramping DoT",
      "SELECT opcode, base_amount, max_amount, resource_mode, per_tick_increment"
      " FROM spell_effect WHERE spell_id=(SELECT id FROM spell WHERE name_canonical='splurt')",
      lambda r: len(r) == 1 and r[0][0] == "HP" and r[0][1] == -11.0 and r[0][2] == -203.0
                and r[0][3] == "PER_TICK" and r[0][4] == 12.0)
check("Spirit of Sharik warder stat block",
      "SELECT DISTINCT pet_classes, level, hp, max_damage, dual_wields FROM pet_stat_block"
      " WHERE summoning_spell_name='Spirit of Sharik'",  # DISTINCT: page cached twice in fixtures
      lambda r: r == [("BST/WAR", 9, 380, 10, "Rarely")])
check("Illusion: Human flags",
      "SELECT is_illusion FROM spell WHERE name_canonical='illusion: human'",
      lambda r: r == [(1,)])

tot = con.execute("SELECT COUNT(*) FROM spell_effect WHERE is_stacking_rule=0").fetchone()[0]
unp = con.execute("SELECT COUNT(*) FROM spell_effect WHERE opcode='UNPARSED'").fetchone()[0]
print(f"\neffect rows={tot} unparsed={unp}")
for row in con.execute("SELECT s.name, e.raw_text FROM spell_effect e JOIN spell s ON s.id=e.spell_id"
                       " WHERE e.opcode IN ('UNPARSED','UNKNOWN_STAT')"):
    print("  needs-look:", row[0], "->", row[1])

print(f"\n{'ALL CHECKS PASSED' if not fails else str(len(fails)) + ' CHECKS FAILED'}")
sys.exit(1 if fails else 0)
