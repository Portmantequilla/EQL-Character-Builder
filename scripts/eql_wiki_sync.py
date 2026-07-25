#!/usr/bin/env python3
"""
eql_wiki_sync.py - Mirror EverQuest Legends wiki (eqlwiki.com) gear + mob data
into a local SQLite DB you can theorycraft against, with incremental updates.

WHY THIS EXISTS
  The wiki stores every item/mob as a MediaWiki template ({{Itempage}}, {{Namedmobpage}}).
  This tool pulls the raw wikitext via the API, parses the templates into normalized
  tables, and records the era so "out of era" items come along and can be updated later.

DESIGN
  fetch  -> pull wikitext from the API (needs `requests`; run on YOUR machine)
  parse  -> pure-stdlib template parsing (no network) so it can run anywhere
  load   -> upsert into SQLite (eql.db)
  Separation means you can re-parse saved raw JSON without re-downloading.

USAGE
  python eql_wiki_sync.py map-categories          # refresh categories.json
  python eql_wiki_sync.py sync --gear --mobs       # full pull of gear + mobs (default)
  python eql_wiki_sync.py sync --category "Monk Equipment"
  python eql_wiki_sync.py sync --incremental       # only pages changed since last sync
  python eql_wiki_sync.py sync-spells              # full pull of spell + BST pet pages -> spell tables
  python eql_wiki_sync.py load --from-raw raw/legends_only.json   # parse+load a saved dump
  python eql_wiki_sync.py export                   # write per-category JSON to exports/
  python eql_wiki_sync.py theorycraft SHD MNK SHM  # gear + transferable mods for a build

Only the fetch paths import `requests`; parse/load/theorycraft are stdlib-only.
"""
import os, re, sys, json, sqlite3, time, argparse, datetime

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
API  = "https://eqlwiki.com/api.php"
DB   = os.environ.get("EQL_DB", os.path.join(BASE, "db", "eql.db"))
RAW  = os.path.join(BASE, "raw")
EXP  = os.path.join(BASE, "exports")
UA   = "EQL-Wiki-Sync/1.0 (personal theorycraft tool)"

# Categories the sync pulls by default (gear + mobs). Era categories are NOT filtered:
# we download everything and store the era, per the "grab out-of-era too" requirement.
GEAR_CATEGORIES = ["Shadow Knight Equipment", "Monk Equipment", "Shaman Equipment",
                   "Items"]  # "Items" is the superset; the class cats let you pull a build slice fast
MOB_CATEGORIES  = ["Named Mobs", "NPCs"]

ERA_TEMPLATES = {  # {{X}} era tags -> era name
    "Classic Era": "Classic", "Kunark Era": "Kunark", "Velious Era": "Velious",
    "Fear Era": "Fear", "Hate Era": "Hate", "Sky Era": "Sky", "Temple Era": "Temple",
    "Paineel Era": "Paineel", "Chardok Revamp Era": "Chardok Revamp",
    "FearHateRevamp Era": "FearHateRevamp", "Epic Quests Era": "Epic Quests",
    "Legends Only": "Legends Only",
}
STAT_KEYS = ["STR","STA","AGI","DEX","WIS","INT","CHA","HP","MANA","END","AC","ATK",
             "HP REGEN","MANA REGEN","ENDURANCE"]
SAVE_KEYS = ["SV FIRE","SV COLD","SV MAGIC","SV POISON","SV DISEASE","SV CORRUPTION",
             "SV FIRE","SV DISEASE"]
CLASS_ABBR = ["WAR","CLR","PAL","RNG","SHD","DRU","MNK","BRD","ROG","SHM","NEC","WIZ",
              "MAG","ENC","BST","BER"]
# seeded class table (plan Group 12): stable ids 1..16 in CLASS_ABBR order
CLASS_SEED = [("WAR","Warrior","MELEE"),("CLR","Cleric","PRIEST"),("PAL","Paladin","HYBRID"),
              ("RNG","Ranger","HYBRID"),("SHD","Shadow Knight","HYBRID"),("DRU","Druid","PRIEST"),
              ("MNK","Monk","MELEE"),("BRD","Bard","HYBRID"),("ROG","Rogue","MELEE"),
              ("SHM","Shaman","PRIEST"),("NEC","Necromancer","CASTER"),("WIZ","Wizard","CASTER"),
              ("MAG","Magician","CASTER"),("ENC","Enchanter","CASTER"),("BST","Beastlord","HYBRID"),
              ("BER","Berserker","MELEE")]
CLASS_ID = {abbr: i + 1 for i, (abbr, _n, _a) in enumerate(CLASS_SEED)}
CLASS_NAME2ABBR = {name: abbr for abbr, name, _a in CLASS_SEED}
CLASS_NAME2ABBR["Shadowknight"] = "SHD"
INSTRUMENT_SKILLS = {"singing": "SINGING", "percussion": "PERCUSSION", "stringed": "STRINGED",
                     "brass": "BRASS", "wind": "WIND"}

# ----------------------------------------------------------------------------- DB
SCHEMA = """
CREATE TABLE IF NOT EXISTS items(
  pageid INTEGER PRIMARY KEY, name TEXT UNIQUE, icon_id INTEGER,
  slot TEXT, weapon_skill TEXT, atk_delay INTEGER, dmg INTEGER, ac INTEGER,
  haste_pct INTEGER, worn_effect TEXT, focus_effect TEXT, click_effect TEXT,
  flags TEXT, era TEXT, notes TEXT, merchant_value TEXT, raw_statsblock TEXT,
  updated TEXT
);
CREATE TABLE IF NOT EXISTS item_stats(pageid INTEGER, stat TEXT, value INTEGER,
  PRIMARY KEY(pageid, stat));
CREATE TABLE IF NOT EXISTS item_classes(pageid INTEGER, class TEXT,
  PRIMARY KEY(pageid, class));
CREATE TABLE IF NOT EXISTS item_races(pageid INTEGER, race TEXT,
  PRIMARY KEY(pageid, race));
CREATE TABLE IF NOT EXISTS item_categories(pageid INTEGER, category TEXT,
  PRIMARY KEY(pageid, category));
CREATE TABLE IF NOT EXISTS mobs(
  pageid INTEGER PRIMARY KEY, name TEXT UNIQUE, race TEXT, class TEXT, level TEXT,
  zone TEXT, loc TEXT, respawn TEXT, hp TEXT, dmg_per_hit TEXT,
  attacks_per_round TEXT, attack_speed TEXT, special TEXT, era TEXT, updated TEXT);
CREATE TABLE IF NOT EXISTS drops(mob_name TEXT, item_name TEXT, rarity TEXT,
  PRIMARY KEY(mob_name, item_name));
CREATE TABLE IF NOT EXISTS sync_meta(key TEXT PRIMARY KEY, value TEXT);

-- convenience view: one row per (item, class) it can be worn by (ALL is expanded on query)
CREATE VIEW IF NOT EXISTS v_item_class AS
  SELECT i.pageid, i.name, i.slot, i.era, c.class, i.worn_effect, i.focus_effect,
         i.haste_pct, i.ac, i.dmg
  FROM items i JOIN item_classes c ON c.pageid = i.pageid;

-- ---- spell domain (M0; canonical names per docs/character-builder-plan.md section 2.1 Group 3)
CREATE TABLE IF NOT EXISTS class(
  id INTEGER PRIMARY KEY, abbr TEXT NOT NULL UNIQUE, name TEXT NOT NULL UNIQUE,
  archetype TEXT NOT NULL CHECK (archetype IN ('MELEE','PRIEST','CASTER','HYBRID')));
CREATE TABLE IF NOT EXISTS spell(
  id INTEGER PRIMARY KEY,                -- MediaWiki pageid
  name TEXT NOT NULL,
  name_canonical TEXT NOT NULL,          -- casefold, ws-collapsed, curly-quote-normalized
  page_title TEXT,                       -- wiki page title (dedup tie-break vs bad spellname fields)
  icon TEXT, casting_skill TEXT, mana INTEGER, "range" INTEGER,
  casting_time REAL, fizzle_time REAL, recast_time REAL,
  duration_raw TEXT, target_type_raw TEXT, spell_type_raw TEXT,
  is_beneficial INTEGER NOT NULL DEFAULT 0,
  resist_type TEXT CHECK (resist_type IN ('UNRESISTABLE','MAGIC','FIRE','COLD','POISON','DISEASE','VOID')),
  resist_adjust INTEGER,
  era TEXT, era_source TEXT CHECK (era_source IN ('TAG','TABLEERA','CATEGORY','DEFAULT','OVERRIDE')),
  is_npc_only INTEGER NOT NULL DEFAULT 0,
  is_illusion INTEGER NOT NULL DEFAULT 0,
  illusion_race_id INTEGER,
  is_song INTEGER NOT NULL DEFAULT 0,
  role TEXT CHECK (role IN ('PET_SUMMON','CONTROL','DAMAGE','PET_BUFF','BUFF','UTILITY')),
  description TEXT, msg_cast_on_you TEXT, msg_cast_on_other TEXT, msg_wears_off TEXT,
  where_to_obtain_raw TEXT, other_raw TEXT,
  template_name TEXT CHECK (template_name IN ('Spellpage','Spellpagesmart')),
  raw_wikitext TEXT, source_revision INTEGER, updated TEXT);
CREATE UNIQUE INDEX IF NOT EXISTS ux_spell_name_canonical ON spell(name_canonical);
CREATE TABLE IF NOT EXISTS spell_class_level(
  spell_id INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  class_id INTEGER NOT NULL REFERENCES class(id),
  required_class_level INTEGER NOT NULL CHECK (required_class_level BETWEEN 1 AND 99),
     -- wiki carries a few 61/63 spells (beyond the current 50 cap); store true values
  is_autogranted INTEGER NOT NULL DEFAULT 0,
  source_revision INTEGER,
  PRIMARY KEY (spell_id, class_id));
CREATE INDEX IF NOT EXISTS idx_scl_class ON spell_class_level(class_id, required_class_level);
CREATE TABLE IF NOT EXISTS spell_effect(
  id INTEGER PRIMARY KEY,
  spell_id INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  slot_number INTEGER, raw_text TEXT NOT NULL,
  opcode TEXT, stat TEXT, base_amount REAL, max_amount REAL,
  min_caster_level INTEGER, max_caster_level INTEGER,
  caster_level_scaling TEXT NOT NULL DEFAULT 'NONE' CHECK (caster_level_scaling IN ('NONE','LINEAR_ASSUMED')),
  is_percent INTEGER NOT NULL DEFAULT 0,
  resource_mode TEXT CHECK (resource_mode IN ('MAX','CURRENT','PER_TICK')),
  per_tick_increment REAL, tier_scaling_json TEXT,
  instrument_scaled INTEGER NOT NULL DEFAULT 0,
  is_cosmetic INTEGER NOT NULL DEFAULT 0,
  grants_proc_spell_id INTEGER REFERENCES spell(id),
  pet_token TEXT, is_stacking_rule INTEGER NOT NULL DEFAULT 0,
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED' CHECK (verification_status IN
    ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST','MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER);
CREATE INDEX IF NOT EXISTS idx_spell_effect_spell ON spell_effect(spell_id);
CREATE TABLE IF NOT EXISTS spell_target_rule(
  spell_id INTEGER PRIMARY KEY REFERENCES spell(id) ON DELETE CASCADE,
  target_base TEXT NOT NULL CHECK (target_base IN ('SELF','SINGLE','GROUP','PET','CORPSE','AE','UNKNOWN')),
  pet_targetable INTEGER,                -- default derivation, NEEDS_INGAME_TEST (plan V4)
  pet_targetable_status TEXT NOT NULL DEFAULT 'NEEDS_INGAME_TEST' CHECK (pet_targetable_status IN
    ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST','MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  pet_subtype TEXT NOT NULL DEFAULT 'ANY' CHECK (pet_subtype IN ('ANY','SUMMONED_ONLY','CHARMED_ONLY','OWNER_PET_ONLY')),
  target_level_min INTEGER, target_level_max INTEGER,
  excluded_target_types_json TEXT,
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED' CHECK (verification_status IN
    ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST','MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER);
CREATE TABLE IF NOT EXISTS spell_duration_rule(
  spell_id INTEGER PRIMARY KEY REFERENCES spell(id) ON DELETE CASCADE,
  duration_class TEXT NOT NULL CHECK (duration_class IN
    ('INSTANT','PERMANENT','LONG','SHORT','BARD_PULSE','PROC_TRIGGERED','CLICK_TRIGGERED',
     'DISCIPLINE','AURA','MAINTAINED_TOGGLE','UNKNOWN')),
  maintenance_type TEXT CHECK (maintenance_type IN
    ('NORMAL_BUFF','BARD_SONG','BARD_AUTO_PULSE','SHORT_COMBAT_BUFF','PERMANENT_SELF_BUFF')),
  duration_seconds_min INTEGER, duration_seconds_max INTEGER,
  duration_min_caster_level INTEGER, duration_max_caster_level INTEGER,
  tick_count INTEGER,
  duration_formula TEXT NOT NULL DEFAULT 'LINEAR_BY_CASTER_LEVEL',
  recast_time REAL,
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED' CHECK (verification_status IN
    ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST','MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER);
CREATE TABLE IF NOT EXISTS bard_song_rule(
  spell_id INTEGER PRIMARY KEY REFERENCES spell(id) ON DELETE CASCADE,
  cast_time REAL, duration_ticks INTEGER,
  instrument_type TEXT CHECK (instrument_type IN ('PERCUSSION','STRINGED','BRASS','WIND','SINGING','ALL','NONE')),
  instrument_scaling_allowed TEXT NOT NULL DEFAULT 'NO' CHECK (instrument_scaling_allowed IN ('NO','YES','REQUIRED')),
  is_sustainable INTEGER NOT NULL DEFAULT 1,
  minimum_cycle_time REAL, bard_layer INTEGER,
  verification_status TEXT NOT NULL DEFAULT 'PARTIALLY_VERIFIED' CHECK (verification_status IN
    ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST','MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER);
CREATE TABLE IF NOT EXISTS spell_source(
  id INTEGER PRIMARY KEY,
  spell_id INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  source_type TEXT NOT NULL CHECK (source_type IN ('VENDOR','DROP','QUEST','RESEARCH','UNKNOWN')),
  zone_name TEXT, npc_name TEXT, area TEXT, loc TEXT, raw_text TEXT, source_revision INTEGER);
CREATE TABLE IF NOT EXISTS spell_item_source(
  spell_id INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  item_name TEXT NOT NULL, PRIMARY KEY (spell_id, item_name));
CREATE TABLE IF NOT EXISTS spell_categories(
  spell_id INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  category TEXT NOT NULL, PRIMARY KEY (spell_id, category));
CREATE TABLE IF NOT EXISTS spell_stacking_rule(
  id INTEGER PRIMARY KEY,
  spell_id INTEGER NOT NULL REFERENCES spell(id) ON DELETE CASCADE,
  rule_type TEXT NOT NULL CHECK (rule_type IN
    ('BLOCK_IF_PRESENT','OVERWRITE_IF_LOWER','OVERWRITE_ALWAYS','BLOCK_IF_HIGHER',
     'MUTUALLY_EXCLUSIVE','STACKS_EXPLICITLY','ILLUSION_EXCLUSIVE','EFFECT_SLOT_CONFLICT')),
  affected_spell_id INTEGER REFERENCES spell(id),
  affected_buff_line_id INTEGER,         -- FK lands with buff_line at the Buff Lines import step
  affected_effect_slot INTEGER, affected_effect_opcode TEXT,
  comparison_operator TEXT CHECK (comparison_operator IN ('<','<=','=','>=','>','!=')),
  comparison_value REAL,
  priority INTEGER NOT NULL DEFAULT 0, order_dependent INTEGER NOT NULL DEFAULT 0,
  source_type TEXT NOT NULL CHECK (source_type IN
    ('WIKI_SLOT_ROW','WIKI_PROSE','WIKI_CATEGORY','BUFF_LINES_PAGE','OVERRIDE','INGAME_TEST')),
  verified INTEGER NOT NULL DEFAULT 0, source_revision INTEGER, notes TEXT);
CREATE TABLE IF NOT EXISTS spell_pet_summon(
  spell_id INTEGER PRIMARY KEY REFERENCES spell(id) ON DELETE CASCADE,
  pet_archetype_id INTEGER,              -- linked at M1 when pet_archetype lands
  pet_classes TEXT,                      -- 'WAR/SHD' from the spell page other-block
  summon_token TEXT, base_pet_level INTEGER,
  base_level_source TEXT CHECK (base_level_source IN ('SUMMONEDPETPAGE','OTHER_BLOCK','TOKEN','OVERRIDE')),
  base_pet_level_status TEXT NOT NULL DEFAULT 'NEEDS_INGAME_TEST' CHECK (base_pet_level_status IN
    ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST','MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  pet_hp TEXT, pet_hp_numeric INTEGER, pet_max_hit INTEGER, pet_harm_touch INTEGER, pet_lifetap INTEGER,
  source_revision INTEGER);
CREATE TABLE IF NOT EXISTS pet_stat_block(   -- {{Summonedpetpage}} rows (BST warders)
  page_pageid INTEGER PRIMARY KEY,
  summon_spell_id INTEGER REFERENCES spell(id),
  summoning_spell_name TEXT,             -- linkage key until resolved in finalize_spells
  pet_archetype_id INTEGER,
  pet_classes TEXT,
  level INTEGER, hp INTEGER, hp_regen INTEGER, mana INTEGER, mana_regen INTEGER,
  mitigation INTEGER, avoidance INTEGER, offense INTEGER, accuracy INTEGER,
  str INTEGER, sta INTEGER, agi INTEGER, dex INTEGER, wis INTEGER, intel INTEGER, cha INTEGER,
  max_damage INTEGER, dual_wields TEXT, abilities TEXT, innate_spells_raw TEXT,
  verification_status TEXT NOT NULL DEFAULT 'WIKI_CONFIRMED' CHECK (verification_status IN
    ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST','MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  source_revision INTEGER);
CREATE VIEW IF NOT EXISTS v_spell_class AS
  SELECT s.id, s.name, c.abbr AS class, l.required_class_level, l.is_autogranted,
         s.target_type_raw, s.spell_type_raw, s.is_beneficial, s.is_song, s.role, s.era, s.is_npc_only
  FROM spell s JOIN spell_class_level l ON l.spell_id = s.id JOIN class c ON c.id = l.class_id;
"""

def db():
    os.makedirs(os.path.dirname(DB), exist_ok=True)
    con = sqlite3.connect(DB)
    con.executescript(SCHEMA)
    for i, (abbr, name, arch) in enumerate(CLASS_SEED, 1):
        con.execute("INSERT OR IGNORE INTO class(id,abbr,name,archetype) VALUES(?,?,?,?)",
                    (i, abbr, name, arch))
    con.commit()
    return con

# --------------------------------------------------------------------------- parse
def parse_template_fields(text, tname):
    """Extract {{tname | k = v ...}} into dict. Handles multiline values, nested [[..]]."""
    m = re.search(r"\{\{\s*" + re.escape(tname) + r"\b", text)
    if not m: return None
    # find matching braces from m.start()
    i = m.start(); depth = 0; end = None
    while i < len(text):
        if text[i:i+2] == "{{": depth += 1; i += 2; continue
        if text[i:i+2] == "}}":
            depth -= 1; i += 2
            if depth == 0: end = i; break
            continue
        i += 1
    block = text[m.start():end] if end else text[m.start():]
    inner = block[2 + len(tname):-2]  # strip {{tname ... }}
    # split on top-level pipes (ignore pipes inside [[ ]] or nested {{ }})
    parts, buf, d1, d2 = [], [], 0, 0
    for ch in inner:
        if ch == "[" : d1 += 1
        elif ch == "]": d1 = max(0, d1-1)
        elif ch == "{": d2 += 1
        elif ch == "}": d2 = max(0, d2-1)
        if ch == "|" and d1 == 0 and d2 == 0:
            parts.append("".join(buf)); buf = []
        else:
            buf.append(ch)
    parts.append("".join(buf))
    fields = {}
    for p in parts[1:]:
        if "=" in p:
            k, v = p.split("=", 1)
            fields[k.strip().lower()] = v.strip()
    return fields

def parse_categories(text):
    return [c.strip() for c in re.findall(r"\[\[Category:\s*([^\]|]+)", text)]

def detect_era(text, cats):
    for tmpl, era in ERA_TEMPLATES.items():
        if re.search(r"\{\{\s*" + re.escape(tmpl) + r"\s*\}\}", text): return era
    for c in cats:
        if c in ERA_TEMPLATES: return ERA_TEMPLATES[c]
        if c.endswith(" Era"): return c[:-4].strip()
    return None

SLOT_TOKENS = {"EAR","HEAD","FACE","NECK","SHOULDERS","ARMS","BACK","WRIST",
               "RANGE","HANDS","PRIMARY","SECONDARY","FINGER","CHEST","LEGS",
               "FEET","WAIST","AMMO","CHARM"}

def parse_statsblock(sb):
    """Return dict with slot, weapon_skill, atk_delay, dmg, ac, haste, effects, stats, classes, races, flags."""
    out = {"stats": {}, "classes": [], "races": [], "flags": [],
           "slot": None, "weapon_skill": None, "atk_delay": None, "dmg": None,
           "ac": None, "haste_pct": None, "worn_effect": None, "click_effect": None,
           "focus_effect": None}
    # normalize <br> to newlines
    lines = re.split(r"<br\s*/?>|\n", sb)
    for ln in lines:
        s = ln.strip().rstrip(",")
        if not s: continue
        low = s.lower()
        if low.startswith("slot:"):
            # first 'Slot:' wins; ornamentation-socket lines are not wear slots
            # (56828 'Boots of the Long Road' has 'Slot: FEET' then
            #  'Slot: Ornamentation: empty' which used to clobber the real one)
            val = s.split(":",1)[1].strip()
            if out["slot"] is None and not val.lower().startswith("ornamentation"):
                out["slot"] = val
            continue
        if low.startswith("class:"):
            out["classes"] = re.findall(r"[A-Z]{3}", s.split(":",1)[1]); continue
        if low.startswith("race:"):
            out["races"] = [r.strip() for r in re.split(r"[ ,]+", s.split(":",1)[1].strip()) if r.strip()]; continue
        if low.startswith("skill:"):
            mk = re.search(r"Skill:\s*(.+?)\s+Atk Delay:\s*(\d+)", s, re.I)
            if mk:
                out["weapon_skill"] = mk.group(1).strip(); out["atk_delay"] = int(mk.group(2))
            else:
                out["weapon_skill"] = s.split(":",1)[1].strip()
            continue
        if low.startswith("dmg:"):
            mm = re.search(r"DMG:\s*(\d+)", s, re.I)
            if mm: out["dmg"] = int(mm.group(1))
            continue
        if low.startswith("haste"):
            mm = re.search(r"([+-]?\d+)\s*%", s)
            if mm: out["haste_pct"] = int(mm.group(1))
            continue
        # effect lines: bare 'Effect:' plus the newer (5xxxx pageids)
        # 'Click Effect:' / 'Worn Effect:' / 'Focus Effect:' / 'Combat Effect:'
        # / 'Proc Effect:' prefixed forms
        me = re.match(r"(?:(click|worn|focus|combat|proc)\s+)?effect\s*:\s*(.+)$", s, re.I)
        if me:
            kind, val = (me.group(1) or "").lower(), me.group(2).strip()
            if kind == "focus":
                out["focus_effect"] = val
            elif kind == "worn":
                out["worn_effect"] = val
            elif kind in ("click", "combat", "proc"):
                out["click_effect"] = val
            else:  # bare 'Effect:' — route by the '(Worn)' parenthetical
                out["worn_effect" if "worn" in val.lower() else "click_effect"] = val
            continue
        # bare slot line with no 'Slot:' prefix (57049 writes just 'Wrist'):
        # a line made solely of canonical slot tokens is the slot
        if out["slot"] is None and ":" not in s:
            toks = [t.strip(".,;") for t in re.split(r"[\s,/]+", s.upper()) if t.strip(".,;")]
            if toks and all(t in SLOT_TOKENS for t in toks):
                out["slot"] = s
                continue
        # flag line (no colon): Lore Equipped, Attunable, Quest, Magic Item, No Drop...
        if ":" not in s and any(w in low for w in
                ("attunable","lore","quest","magic","no drop","temporary","expendable","augment")):
            out["flags"] += [f.strip() for f in s.split(",") if f.strip()]
            continue
        # stat / save pairs: "AC: 6 END: 5", "SV FIRE: 20", "INT: 2 DEX: 3 MANA: 10"
        STAT_RE = (r"(SV\s+\w+|HP\s+REGEN|MANA\s+REGEN|STR|STA|AGI|DEX|WIS|INT|CHA|"
                   r"HP|MANA|ENDURANCE|END|ATK|AC):\s*([+-]?\d+)")
        for mm in re.finditer(STAT_RE, s, re.I):
            key = re.sub(r"\s+", " ", mm.group(1).upper().strip()); val = int(mm.group(2))
            if key == "AC": out["ac"] = val
            else: out["stats"][key] = val
    return out

def parse_item(pageid, title, text):
    f = parse_template_fields(text, "Itempage")
    if f is None: return None
    cats = parse_categories(text)
    sb = parse_statsblock(f.get("statsblock",""))
    return {
        "pageid": pageid, "name": f.get("itemname", title).strip() or title,
        "icon_id": int(f["lucy_img_id"]) if f.get("lucy_img_id","").strip().isdigit() else None,
        "slot": sb["slot"], "weapon_skill": sb["weapon_skill"], "atk_delay": sb["atk_delay"],
        "dmg": sb["dmg"], "ac": sb["ac"], "haste_pct": sb["haste_pct"],
        "worn_effect": sb["worn_effect"], "click_effect": sb["click_effect"],
        # template field wins; statsblock 'Focus Effect:' line fills when empty
        "focus_effect": (f.get("focus_effect") or sb["focus_effect"] or None),
        "flags": ", ".join(sb["flags"]) or None, "era": detect_era(text, cats),
        "notes": (f.get("notes") or None), "merchant_value": (f.get("merchant_value") or None),
        "raw_statsblock": f.get("statsblock","").strip(),
        "stats": sb["stats"], "classes": sb["classes"], "races": sb["races"],
        "categories": cats,
        "dropsfrom": re.findall(r"\[\[([^\]|]+)\]\]", f.get("dropsfrom","")),
    }

def parse_mob(pageid, title, text):
    f = parse_template_fields(text, "Namedmobpage")
    if f is None: return None
    cats = parse_categories(text)
    loot = re.findall(r"\{\{:([^}|]+?)\}\}", f.get("known_loot",""))
    # try to grab rarity in parens near each loot line
    rar = {}
    for ln in f.get("known_loot","").split("</li>"):
        mm = re.search(r"\{\{:([^}|]+?)\}\}.*?\(([^)]+)\)", ln)
        if mm: rar[mm.group(1).strip()] = mm.group(2).strip()
    return {
        "pageid": pageid, "name": (f.get("name") or title).strip(),
        "race": f.get("race"), "class": re.sub(r"\[\[|\]\]","", f.get("class","")) or None,
        "level": f.get("level"), "zone": re.sub(r"\[\[|\]\]","", f.get("zone","")) or None,
        "loc": f.get("location"), "respawn": f.get("respawn_time"),
        "hp": f.get("hp"), "dmg_per_hit": f.get("damage_per_hit"),
        "attacks_per_round": f.get("attacks_per_round"), "attack_speed": f.get("attack_speed"),
        "special": f.get("special"), "era": detect_era(text, cats),
        "loot": [(n.strip(), rar.get(n.strip())) for n in loot],
    }

# -------------------------------------------------------------------- spell parse
def canonical_name(s):
    # backtick folds to apostrophe: the wiki mixes 'Turgur`s'/'Turgur's' for the same spell
    s = (s or "").replace("’", "'").replace("`", "'")
    return re.sub(r"\s+", " ", s).strip().rstrip(".").casefold()

def strip_links(s):
    return re.sub(r"\[\[(?:[^\]|]*\|)?([^\]|]*)\]\]", r"\1", s or "")

def split_top_pipes(s):
    """Split on pipes that are not inside [[..]] or {{..}} (same rules as template fields)."""
    parts, buf, d1, d2 = [], [], 0, 0
    for ch in s:
        if ch == "[": d1 += 1
        elif ch == "]": d1 = max(0, d1 - 1)
        elif ch == "{": d2 += 1
        elif ch == "}": d2 = max(0, d2 - 1)
        if ch == "|" and d1 == 0 and d2 == 0:
            parts.append("".join(buf)); buf = []
        else:
            buf.append(ch)
    parts.append("".join(buf))
    return parts

def iter_templates(text, name_pattern):
    """Yield (matched_name, inner_text) for every {{name ...}} template, brace-aware."""
    for m in re.finditer(r"\{\{\s*(" + name_pattern + r")\b", text or ""):
        i = m.start(); depth = 0; end = None
        while i < len(text):
            if text[i:i+2] == "{{": depth += 1; i += 2; continue
            if text[i:i+2] == "}}":
                depth -= 1; i += 2
                if depth == 0: end = i; break
                continue
            i += 1
        if end:
            yield m.group(1), text[m.start()+2:end-2]

STAT_OPCODES = {  # effect-text stat name -> (opcode, resource hint)
    "max hitpoints": ("MAX_HP", "MAX"), "max hit points": ("MAX_HP", "MAX"),
    "hitpoints": ("HP", None), "hit points": ("HP", None), "hp": ("HP", None),
    "mana": ("MANA", None), "max mana": ("MAX_MANA", "MAX"),
    "strength": ("STR", None), "str": ("STR", None), "stamina": ("STA", None), "sta": ("STA", None),
    "agility": ("AGI", None), "agi": ("AGI", None), "dexterity": ("DEX", None), "dex": ("DEX", None),
    "wisdom": ("WIS", None), "wis": ("WIS", None), "intelligence": ("INT", None), "int": ("INT", None),
    "charisma": ("CHA", None), "cha": ("CHA", None),
    "armor class": ("AC", None), "ac": ("AC", None),
    "attack speed": ("HASTE", None), "melee haste": ("HASTE", None),
    "attack": ("ATK", None), "atk": ("ATK", None), "attack rating": ("ATK", None),
    "movement speed": ("MOVE_SPEED", None), "movement rate": ("MOVE_SPEED", None),
    "damage shield": ("DAMAGE_SHIELD", None), "endurance": ("END", None),
    "spell haste": ("SPELL_HASTE", None),
    "current hit points": ("HP", "CURRENT"), "current hitpoints": ("HP", "CURRENT"),
    "hitpoints v2": ("HP", None), "current mana": ("MANA", "CURRENT"),
    "max hp": ("MAX_HP", "MAX"), "hp regen": ("HP_REGEN", "PER_TICK"),
    "stamina loss": ("STAMINA_LOSS", None), "hate": ("HATE", None),
    "agro multiplier": ("HATE_MULT", None), "aggro multiplier": ("HATE_MULT", None),
    "aggro radius": ("AGGRO_RADIUS", None), "agro radius": ("AGGRO_RADIUS", None),
    "magnification": ("MAGNIFICATION", None), "player size": ("PLAYER_SIZE", None),
    "absorb damage": ("ABSORB_DAMAGE", None), "absorb magic damage": ("ABSORB_MAGIC", None),
    "spell mana cost": ("FOCUS_MANA_COST", None), "spell damage": ("FOCUS_SPELL_DAMAGE", None),
    "spell duration": ("FOCUS_SPELL_DURATION", None), "spell range": ("FOCUS_SPELL_RANGE", None),
    "healing": ("FOCUS_HEALING", None), "incoming healing": ("INCOMING_HEALING", None),
    "chance of using reagent": ("FOCUS_REAGENT_CHANCE", None),
    "effective casting level": ("EFFECTIVE_CASTING_LEVEL", None),
    "faction": ("FACTION", None), "mr": ("RESIST_MAGIC", None),
    "chance to reflect spell": ("SPELL_REFLECT", None), "chance to hit": ("CHANCE_TO_HIT", None),
    "singing skill": ("SKILL_SINGING", None), "hp when cast": ("HP_WHEN_CAST", "CURRENT"),
    "atk power": ("ATK", None), "pet size": ("PET_SIZE", None), "haste": ("HASTE", None),
}
BARE_EFFECTS = {"root": "ROOT", "levitate": "LEVITATE", "levitation": "LEVITATE",
    "mesmerize": "MEZ", "charm": "CHARM", "fear": "FEAR", "stun": "STUN", "gate": "GATE",
    "invisibility": "INVIS", "see invisible": "SEE_INVIS", "water breathing": "WATER_BREATHING",
    "enduring breath": "WATER_BREATHING", "infravision": "INFRAVISION",
    "ultravision": "ULTRAVISION", "bind affinity": "BIND", "identify": "IDENTIFY"}
# prefix-matched flag effects, longest/most-specific first (checked after paren/cap stripping)
PREFIX_EFFECTS = [
    ("cancel magic", "DISPEL"), ("see invisible", "SEE_INVIS"),
    ("water breathing", "WATER_BREATHING"), ("enduring breath", "WATER_BREATHING"),
    ("bind sight", "BIND_SIGHT"), ("bind affinity", "BIND"),
    ("reaction radius", "REACTION_RADIUS"), ("frenzy radius", "FRENZY_RADIUS"),
    ("feign death", "FEIGN_DEATH"), ("destroy target", "DESTROY"),
    ("summon corpse", "SUMMON_CORPSE"), ("locate corpse", "LOCATE_CORPSE"),
    ("sense undead", "SENSE"), ("sense animals", "SENSE"), ("sense summoned", "SENSE"),
    ("spinstun", "STUN"), ("memblur", "MEMBLUR"), ("mesmerize", "MEZ"), ("charm", "CHARM"),
    ("fear", "FEAR"), ("pacify", "PACIFY"), ("lull", "PACIFY"), ("calm", "PACIFY"),
    ("root", "ROOT"), ("snare", "SNARE"), ("levitat", "LEVITATE"),
    ("ultravision", "ULTRAVISION"), ("infravision", "INFRAVISION"),
    ("invisib", "INVIS"), ("gate", "GATE"), ("identify", "IDENTIFY"), ("shrink", "SHRINK"),
    ("blindness", "BLIND"), ("blind", "BLIND"), ("silence", "SILENCE"),
    ("improved invisib", "INVIS"), ("invulnerability", "INVULNERABILITY"),
    ("eye of", "EYE_OF_ZOMM"), ("shadowstep", "SHADOWSTEP"), ("npc: gate", "SHADOWSTEP"),
    ("random teleport", "TELEPORT"), ("teleports", "TELEPORT"),
    ("toss up", "TOSS_UP"), ("pushback", "PUSHBACK"), ("make fragile", "MAKE_FRAGILE"),
    ("incite hatred", "HATE"), ("true north", "TRUE_NORTH"), ("cast light", "LIGHT"),
    ("grow", "PLAYER_SIZE"), ("reclaim energy", "RECLAIM_ENERGY"), ("sacrifice", "SACRIFICE"),
    ("sentinel", "SENTINEL"), ("set targeted proximity alert", "SENTINEL"),
    ("grants ultravision", "ULTRAVISION"), ("call pet", "CALL_PET"),
    ("voice graft", "VOICE_GRAFT"), ("stop rain", "STOP_RAIN"),
    ("complete heal", "COMPLETE_HEAL"), ("causes you to spin", "STUN"),
    ("sustenance", "SUSTENANCE"), ("heals anyone that hits", "HEAL_SHIELD"),
    ("pet power increase", "PET_POWER"), ("grant", "GRANT_ABILITY"),
    ("need more information", "NOTE"), ("daytime only", "NOTE"), ("night time only", "NOTE"),
    ("nighttime only", "NOTE"), ("unresistable", "NOTE"), ("ticks in order", "NOTE"),
]
# informational pseudo-rows the wiki puts in effect slots; recognized, not stat effects
# ('Recourse' deliberately NOT here: 'Recourse Effect: Increase Caster Mana...' is real math)
NOTE_ROW_RE = re.compile(
    r"^(Duration|Cooldown|Recast|Requires?|Level Cap|Resist|Max Hits?|Consumes?|"
    r"Reagent|Blessing|Push(?:back)?|Range|Notes?)\b\s*[: ]", re.I)
RESIST_MAP = {"unresistable": "UNRESISTABLE", "magic": "MAGIC", "fire": "FIRE", "cold": "COLD",
              "poison": "POISON", "disease": "DISEASE", "void": "VOID"}

def _stat_opcode(stat_text):
    s = re.sub(r"\s+", " ", stat_text or "").strip().lower()
    s = re.sub(r"\s*\([^)]*\)$", "", s)      # 'Spell Damage (Before DoT Crit)' -> 'spell damage'
    s = re.sub(r"\s+v\d+$", "", s)           # 'Haste v2' -> 'haste'
    s = re.sub(r"^(target|caster)\s+|\s+of\s+target$", "", s)  # 'Target Mana' / 'Caster Mana'
    if s in STAT_OPCODES: return STAT_OPCODES[s]
    if "resist" in s:
        if "all" in s: return ("RESIST_ALL", None)
        for el in ("magic", "fire", "cold", "poison", "disease"):
            if el in s: return ("RESIST_" + el.upper(), None)
    m = re.match(r"^(\w+) counter$", s)
    if m: return (m.group(1).upper() + "_COUNTER", None)
    return ("UNKNOWN_STAT", None)

def parse_effect_text(raw):
    """Grammar from recon-spells section 5. Returns effect dict; 'stack' set for Stacking: rows."""
    s = re.sub(r"'''", "", raw or "").strip()
    ef = dict(opcode="UNPARSED", stat=None, base=None, max=None, minlvl=None, maxlvl=None,
              scaling="NONE", pct=0, resource=None, ptinc=None, cosmetic=0,
              pet_token=None, illusion=None, stack=None)
    m = re.match(r"Stacking:\s*(Block new spell|Overwrite existing spell)\s+if slot\s+(\d+)\s+is"
                 r"\s+(?:effect\s+)?'?(.+?)'?\s*(?:and\s+(<=|>=|!=|[<>=]|Less Than|Greater Than)"
                 r"\s*([\d,.]+))?\s*$", s, re.I)
    if m:
        verb = m.group(1).lower()
        cmp_ = {"less than": "<", "greater than": ">"}.get((m.group(4) or "").lower(), m.group(4))
        rule = "BLOCK_IF_PRESENT" if verb.startswith("block") else \
               ("OVERWRITE_IF_LOWER" if cmp_ == "<" else "OVERWRITE_ALWAYS")
        ef.update(opcode="STACKING_RULE",
                  stack=dict(rule_type=rule, slot=int(m.group(2)), opcode=_stat_opcode(m.group(3))[0],
                             cmp=cmp_, value=float(m.group(5).replace(",", "")) if m.group(5) else None))
        return ef
    m = re.match(r"^Recourse(?:\s+Effect)?\s*:\s*(.+)$", s, re.I)
    if m:  # a real effect that lands on the caster, not an informational note
        inner = parse_effect_text(m.group(1))
        inner["opcode"] = "RECOURSE_" + (inner["opcode"] or "UNPARSED")
        return inner
    if re.match(r"^Summon\b", s, re.I):
        mp = re.match(r"^Summon(?:\s+[\w' ]+?)?\s+(?:Pet|Warder)\s*:?\s*(\S+)?$", s, re.I)
        if mp:
            ef["opcode"] = "SUMMON_PET"
            ef["pet_token"] = mp.group(1).strip() if mp.group(1) else None
            return ef
        if re.match(r"^Summon Corpse", s, re.I):
            ef["opcode"] = "SUMMON_CORPSE"; return ef
        ef["opcode"] = "SUMMON_OTHER"; ef["stat"] = s; return ef
    m = re.match(r"^Illusion\b\s*(?:\(race\s*)?[:#]?\s*#?\s*(\d+)?", s, re.I)
    if m and s.lower().startswith("illusion"):
        ef["opcode"] = "ILLUSION"; ef["cosmetic"] = 1
        if m.group(1): ef["illusion"] = int(m.group(1))
        else: ef["stat"] = re.sub(r"^Illusion[:\s]*", "", s, flags=re.I).strip() or None
        return ef
    if re.match(r"^Limit\b", s, re.I):  # focus-effect limit rows
        ef["opcode"] = "FOCUS_LIMIT"; ef["stat"] = re.sub(r"^Limit\s*", "", s, flags=re.I)
        return ef
    m = re.match(r"^(Teleport|Translocate|Evacuate|Succor)\b(?:\s+group)?\s*(?:to\s+(.+?))?$", s, re.I)
    if m:
        ef["opcode"] = "TELEPORT"
        dest = strip_links(re.sub(r"\{\{Loc\|([^}|]*)\|[^}]*\}\}", r"\1", m.group(2) or "")).strip()
        ef["stat"] = dest or None
        return ef
    m = re.match(r"^Add\s+(?:Melee\s+|Ranged\s+|Skill\s+|Defensive\s+)?(?:Proc(?:_\w+)?|effect)"
                 r"[:\s]+\s*(.+)$", s, re.I)
    if m:
        body = m.group(1)
        ml = re.search(r"\[\[([^\]|]+)", body)  # prefer the LINK TARGET over piped display text
        if ml:
            name = ml.group(1)
        else:
            name = re.sub(r"\{\{SpellHoverLink\|([^}|]*)[^}]*\}\}", r"\1", body)
            name = strip_links(name)
        name = re.sub(r"<[^>]+>", "", name)                      # drop html spans
        name = re.sub(r"\[[^\]]*\]", "", name)                   # drop [note] suffix
        name = re.sub(r"\s+with\s+[\d.]+%\s*Rate\s*Mod.*$", "", name, flags=re.I)
        name = name.replace("'''", "").strip().rstrip(":").strip()
        ef["opcode"] = "ADD_PROC"; ef["stat"] = name or None
        return ef
    m = re.match(r"^Mitigate\s+(.+?)\s+by\s+([\d.]+)\s*%(?:,\s*([\d,]+)\s*total)?", s, re.I)
    if m:
        ef.update(opcode="MITIGATION", stat=m.group(1).strip(), base=float(m.group(2)), pct=1,
                  max=float(m.group(3).replace(",", "")) if m.group(3) else None)
        return ef
    m = re.match(r"^Absorb\s+(?:Hit\s+up\s+to\s+)?([\d,]+)\s+damage(?:\s+per\s+hit)?"
                 r"(?:\s*\(up to ([\d,]+) total\))?", s, re.I)
    if m:
        ef.update(opcode="ABSORB_DAMAGE", base=float(m.group(1).replace(",", "")),
                  max=float(m.group(2).replace(",", "")) if m.group(2) else None)
        return ef
    m = re.match(r"^UNKNOWN CALC\s+(\d+)\s+base\s+([-\d.]+)\s+max\s+([-\d.]+)\s+attrib\s+(.+)$", s, re.I)
    if m:
        opcode, res_hint = _stat_opcode(m.group(4))
        ef.update(opcode=opcode if opcode != "UNKNOWN_STAT" else "UNKNOWN_STAT",
                  stat=m.group(4).strip(), base=float(m.group(2)), max=float(m.group(3)),
                  resource=res_hint)
        return ef
    if re.match(r"^Stun\b|^SpinStun\b", s, re.I):
        ef["opcode"] = "STUN"
        md = re.search(r"([\d.]+)\*?\s*s(?:econds?)?\b", s, re.I)  # tolerate '6.0*' footnote star
        if md: ef["base"] = float(md.group(1))
        return ef
    m = re.match(r"^Resurrect\b(?:.*?([\d.]+)%\s+experience)?", s, re.I)
    if m and s.lower().startswith("resurrect"):
        ef["opcode"] = "RESURRECT"
        if m.group(1): ef["base"] = float(m.group(1)); ef["pct"] = 1
        return ef
    m = re.match(r"^Return\s+([\d.]+)%\s+of\s+damage\s+as\s+HP", s, re.I)
    if m:
        ef["opcode"] = "LIFETAP_PCT"; ef["base"] = float(m.group(1)); ef["pct"] = 1
        return ef
    m = re.match(r"^Strikes?\s+heal\s+for\s+([\d.]+)", s, re.I)
    if m:
        ef["opcode"] = "STRIKE_HEAL"; ef["base"] = float(m.group(1))
        return ef
    m = re.match(r"^Interrupt\s+Spell\s*Casting\s*\(?([\d.]+)?%?\)?", s, re.I)
    if m and s.lower().startswith("interrupt"):
        ef["opcode"] = "INTERRUPT"
        if m.group(1): ef["base"] = float(m.group(1)); ef["pct"] = 1
        return ef
    if re.match(r"^Death Save\b", s, re.I):
        ef["opcode"] = "DEATH_SAVE"; ef["stat"] = s
        return ef
    m = re.match(r"^Restores?\s+Up\s+To\s+([\d,]+)\s+HP", s, re.I)
    if m:
        ef["opcode"] = "HP"; ef["resource"] = "CURRENT"
        ef["base"] = float(m.group(1).replace(",", ""))
        return ef
    m = re.match(r"^Increases the power of (\w+) instrument based bard songs by up to ([\d.]+)%", s, re.I)
    if m:
        ef["opcode"] = "INSTRUMENT_MOD"; ef["stat"] = m.group(1).capitalize()
        ef["base"] = float(m.group(2)); ef["pct"] = 1
        return ef
    # NB: connectors after optional groups must be \s* — a greedy \s* inside a satisfied
    # optional prefix eats the space and the engine never backtracks (Splurt case)
    NUM = r"[-+]?[\d,]+(?:\.\d+)?"
    VERB = r"(Increases?|De?creases?|Reduces?)"  # De?crease also catches the 'Derease' wiki typo
    m = re.match(r"^" + VERB + r"\s+(.+?)\s+by\s*(?:up\s+to\s+)?(" + NUM + r")\s*(%)?\s*(?:\(L(\d+)\))?"
                 r"(?:\s*to\s+(" + NUM + r")\s*(%)?\s*(?:\(L(\d+)\))?)?(\s*per\s+tick)?", s, re.I)
    if not m:  # random-range variant: 'Decrease Hitpoints between 96 and 140 per tick.'
        m = re.match(r"^" + VERB + r"\s+(.+?)\s+between\s+(" + NUM + r")()()"
                     r"\s+and\s+(" + NUM + r")()()(.*per\s+tick)?", s, re.I)
    if not m:  # paren-range variant: 'Increase AC (+10 to +15)'
        m = re.match(r"^" + VERB + r"\s+(.+?)\s*\(\s*\+?(" + NUM + r")()()"
                     r"\s*to\s*\+?(" + NUM + r")()()\s*\)()", s, re.I)
    if not m:  # pet-formula variant: 'Increase damage shield by (pet_level+2)' -> value unknown
        mf = re.match(r"^" + VERB + r"\s+(.+?)\s+by\s+\(([^)]*pet_level[^)]*)\)", s, re.I)
        if mf:
            opcode, res_hint = _stat_opcode(mf.group(2))
            ef.update(opcode=opcode, stat=mf.group(2).strip(), resource=res_hint)
            return ef
    if m:
        sign = -1 if m.group(1).lower().startswith(("decrease", "reduce", "drease", "derease")) else 1
        stat_raw = m.group(2).strip(); when_cast = False
        if stat_raw.lower().endswith(" when cast"):
            when_cast = True; stat_raw = stat_raw[:-10].strip()
        opcode, res_hint = _stat_opcode(stat_raw)
        per_tick = bool(m.group(9)) or " per tick" in s.lower()
        ef.update(opcode=("HP_WHEN_CAST" if (when_cast and opcode in ("HP", "MAX_HP")) else opcode),
                  stat=stat_raw, base=sign * float(m.group(3).replace(",", "")),
                  max=sign * float(m.group(6).replace(",", "")) if m.group(6) else None,
                  minlvl=int(m.group(5)) if m.group(5) else None,
                  maxlvl=int(m.group(8)) if m.group(8) else None,
                  pct=1 if (m.group(4) or m.group(7)) else 0)
        if ef["minlvl"] and ef["max"] is not None: ef["scaling"] = "LINEAR_ASSUMED"
        if per_tick: ef["resource"] = "PER_TICK"
        elif when_cast: ef["resource"] = "CURRENT"
        elif res_hint: ef["resource"] = res_hint
        elif ef["opcode"] in ("HP", "MANA"): ef["resource"] = "CURRENT"
        mi = re.search(r"increasing by ([\d.]+) each tick", s, re.I)
        if mi: ef["ptinc"] = float(mi.group(1))
        return ef
    if NOTE_ROW_RE.match(s) or re.match(r"^Requires\s+\d", s, re.I):
        ef["opcode"] = "NOTE"; ef["stat"] = s
        return ef
    # bare flag effects, tolerating '(args)' and 'up to level N' / '(up to LN)' suffixes
    key = re.sub(r"\s+", " ", s).strip().rstrip(".")
    args = None
    mp = re.match(r"^(.*?)\s*\(([^)]*)\)$", key)
    if mp: key, args = mp.group(1).strip(), mp.group(2).strip()
    key = re.sub(r"\s+up to (?:level\s*)?L?\d+$", "", key, flags=re.I).strip().lower()
    if key in BARE_EFFECTS:
        ef["opcode"] = BARE_EFFECTS[key]
    else:
        for pref, op in PREFIX_EFFECTS:
            if key.startswith(pref):
                ef["opcode"] = op; break
    if ef["opcode"] != "UNPARSED" and args and re.match(r"^[-+]?[\d.]+%?$", args):
        ef["base"] = float(args.rstrip("%"))
        if args.endswith("%"): ef["pct"] = 1
    return ef

def _clock_seconds(h, m, sec):
    if sec != "":  # H:MM:SS
        return int(h) * 3600 + int(m) * 60 + int(sec)
    return int(h) * 60 + int(m)  # M:SS

def _span_seconds(s):
    s = s or ""
    clocks = re.findall(r"\b(\d{1,2}):(\d{2})(?::(\d{2}))?\b", s)
    if clocks:  # '2:24:00 (3:36:00)' or ranges '0:03 - 5:00' -> take the LAST (largest) token
        return float(_clock_seconds(*clocks[-1]))
    total, found = 0.0, False
    for m in re.finditer(r"([\d.]+)\s*(hours?|hrs?|minutes?|mins?|seconds?|secs?|ticks?|h|m|s)\b",
                         s, re.I):
        v, u = float(m.group(1)), m.group(2).lower()
        mult = 3600 if u.startswith(("hour", "hr", "h")) else \
               60 if u.startswith(("min", "m")) else 6 if u.startswith("tick") else 1
        total += v * mult; found = True
    return total if found else None

def parse_spell_duration(raw, is_song, is_beneficial, target_base):
    s = (raw or "").strip()
    d = dict(cls="UNKNOWN", maint=None, smin=None, smax=None, lmin=None, lmax=None, ticks=None)
    if not s: return d
    low = s.lower()
    if low.startswith("instant"): d["cls"] = "INSTANT"; return d
    if "permanent" in low or "unlimited" in low:
        d["cls"] = "PERMANENT"
        if is_beneficial and target_base == "SELF": d["maint"] = "PERMANENT_SELF_BUFF"
        return d
    m = re.match(r"^(?:Between\s+)?(.*?)\s*@\s*L\s*(\d+)\s+to\s+(.*?)\s*@\s*L\s*(\d+)", s, re.I)
    if m:
        d["smin"], d["smax"] = _span_seconds(m.group(1)), _span_seconds(m.group(3))
        d["lmin"], d["lmax"] = int(m.group(2)), int(m.group(4))
    else:
        clocks = re.findall(r"\b(\d{1,2}):(\d{2})(?::(\d{2}))?\b", s)
        if len(clocks) >= 2:  # '2:24:00 (3:36:00)' -> base and extended values
            d["smin"] = float(_clock_seconds(*clocks[0]))
            d["smax"] = float(_clock_seconds(*clocks[-1]))
        else:
            d["smin"] = d["smax"] = _span_seconds(s)
    if d["smax"] is None: return d
    d["smin"] = int(round(d["smin"])) if d["smin"] is not None else None
    d["smax"] = int(round(d["smax"]))
    d["ticks"] = d["smax"] // 6 if d["smax"] else None
    if is_song and re.search(r"ticks?", low):
        d["cls"] = "BARD_PULSE"; d["maint"] = "BARD_SONG"; return d
    d["cls"] = "SHORT" if d["smax"] < 120 else "LONG"
    if is_beneficial:
        d["maint"] = "BARD_SONG" if is_song else \
                     ("SHORT_COMBAT_BUFF" if d["cls"] == "SHORT" else "NORMAL_BUFF")
    return d

BODY_TYPE_TARGETS = {"undead": "Undead", "animal": "Animal", "summoned": "Summoned",
                     "plant": "Plant", "uber giants": "Giants", "uber dragons": "Dragons"}

def parse_spell_target(raw):
    """Returns (target_base, target_level_max, only_body_type)."""
    low = (raw or "").strip().lower()
    if not low: return "UNKNOWN", None, None
    m = re.match(r"^single\s*l\s*(\d+)$", low)
    if m: return "SINGLE", int(m.group(1)), None
    if re.search(r"\bae\b", low) or "area" in low or "pbaoe" in low: return "AE", None, None
    if low == "self": return "SELF", None, None
    if low.startswith("single"): return "SINGLE", None, None
    if "group" in low or low.startswith("party"): return "GROUP", None, None
    if low.startswith("pet"): return "PET", None, None
    if low.startswith("corpse"): return "CORPSE", None, None
    if low in BODY_TYPE_TARGETS:  # single-target, restricted to a body type
        return "SINGLE", None, BODY_TYPE_TARGETS[low]
    if low in ("lifetap", "line of sight"): return "SINGLE", None, None
    return "UNKNOWN", None, None

def parse_spell_resist(raw):
    m = re.match(r"^([A-Za-z ]+?)\s*(?:\(\s*([-+]?\d+)\s*\))?$", (raw or "").strip())
    if not m: return None, None
    return RESIST_MAP.get(m.group(1).strip().lower()), (int(m.group(2)) if m.group(2) else None)

def parse_spell_classes(field):
    out = []
    for m in re.finditer(r"\[\[\s*([^\]|]+?)\s*(?:\|[^\]]*)?\]\]\s*[-–—]\s*Level\s*(\d+)"
                         r"\s*(\(\s*Autogranted\s*\))?", field or "", re.I):
        abbr = CLASS_NAME2ABBR.get(m.group(1).strip())
        if abbr: out.append((abbr, int(m.group(2)), 1 if m.group(3) else 0))
    return out

def parse_prose_stacking(desc, is_buff=True):
    rules = []
    # capture stops at sentence/clause breaks so 'though it stacks fine with X, Y' after a
    # semicolon is never swallowed (Speed of the Shissar case)
    for m in re.finditer(r"Does\s+NOT\s+stack\s+with([^.;:]*)", desc or "", re.I):
        seg = re.split(r"\b(?:though|but|however)\b", m.group(1), flags=re.I)[0]
        for t in re.findall(r"\[\[([^\]|]+)", seg):
            t = t.strip()
            if "#" in t or t.lower().startswith("buff lines"):  # section cross-ref, not a spell
                continue
            rules.append(("MUTUALLY_EXCLUSIVE", t))
    # 'replacing [[X]]' is only stacking semantics on actual buffs; on instant/detrimental
    # spells it is progression advice ('recommended ..., replacing Column of Lightning')
    if is_buff:
        for m in re.finditer(r"\breplacing\s+((?:\[\[[^\]]+\]\]\s*(?:,\s*|and\s+)?)+)", desc or "", re.I):
            for t in re.findall(r"\[\[([^\]|]+)", m.group(1)):
                if "#" in t: continue
                rules.append(("OVERWRITE_ALWAYS", t.strip()))
    return rules

def parse_spell_sources(wt):
    out = []
    if not wt: return out
    for _nm, inner in iter_templates(wt, r"SpellWhereRowB?"):
        pos = [strip_links(p).strip() for p in split_top_pipes(inner)[1:]]
        pos += [None] * (4 - len(pos))
        out.append(dict(type="VENDOR", zone=pos[0] or None, npc=pos[1] or None,
                        area=pos[2] or None, loc=pos[3] or None, raw=None))
    cur_zone = None
    for raw_ln in re.split(r"\n|<li>", wt):
        ln = re.sub(r"</?ul[^>]*>|</li>|</?b>|'''", "", raw_ln).strip()
        if not ln or "{{" in ln: continue
        bullet = ln.startswith("*")
        body = ln.lstrip("*").strip()
        if not body: continue
        if not bullet and re.match(r"^\[\[[^\]]+\]\]$", body):
            cur_zone = strip_links(body).strip(); continue
        disp = strip_links(body).strip(); low = disp.lower()
        typ = ("VENDOR" if "vendor" in low else "DROP" if "drop" in low else
               "QUEST" if "quest" in low else "RESEARCH" if "research" in low else None)
        if typ:
            out.append(dict(type=typ, zone=None, npc=None, area=None, loc=None, raw=disp))
        elif body.startswith("[["):
            out.append(dict(type="DROP" if cur_zone else "UNKNOWN", zone=cur_zone,
                            npc=disp, area=None, loc=None, raw=disp))
        else:
            out.append(dict(type="UNKNOWN", zone=None, npc=None, area=None, loc=None, raw=disp))
    return out

def detect_era_src(text, cats, fields):
    for tmpl, era in ERA_TEMPLATES.items():
        if re.search(r"\{\{\s*" + re.escape(tmpl) + r"\s*\}\}", text): return era, "TAG"
    for c in cats:
        if c in ERA_TEMPLATES: return ERA_TEMPLATES[c], "CATEGORY"
        if c.endswith(" Era"): return c[:-4].strip(), "CATEGORY"
    te = (fields.get("tableera") or "").strip()
    if te and "{" not in te: return te, "TABLEERA"
    return "Classic", "DEFAULT"

def _num(v):
    m = re.search(r"-?\d+", v or ""); return int(m.group()) if m else None

def _fnum(v):
    m = re.search(r"-?[\d.]+", v or ""); return float(m.group()) if m else None

def parse_spell(pageid, title, text, revid=None):
    tname = "Spellpagesmart" if re.search(r"\{\{\s*Spellpagesmart\b", text) else "Spellpage"
    f = parse_template_fields(text, tname)
    if f is None: return None
    cats = parse_categories(text)
    name = (f.get("spellname") or title).strip() or title
    classes = parse_spell_classes(f.get("classes", ""))
    npc_only = 1 if re.search(r"NPCs?\s+only", f.get("classes", ""), re.I) else 0
    skill = re.sub(r"^Skill\s+", "", strip_links(f.get("skill", "")).strip()).strip() or None
    skill_key = re.sub(r"[^a-z]+$", "", (skill or "").lower())  # drop dagger footnote marks etc.
    is_song = 1 if skill_key in INSTRUMENT_SKILLS else 0
    spell_type = (f.get("spell_type") or "").strip() or None
    beneficial = 0 if (spell_type or "").strip().lower() == "detrimental" else 1
    resist_type, resist_adjust = parse_spell_resist(f.get("resist", ""))
    target_raw = (f.get("target_type") or "").strip() or None
    tbase, tmax, tonly = parse_spell_target(target_raw or "")
    era, era_src = detect_era_src(text, cats, f)

    effects, stacks, instrument_flag = [], [], None
    slot_rows = []  # (slot_arg, body)
    for _nm, inner in iter_templates(f.get("slots", ""), r"SpellSlotRow(?:Smart)?"):
        pos = [p.strip() for p in split_top_pipes(inner)[1:] if not re.match(r"\s*\w+\s*=", p)]
        if not pos: continue
        slot_rows.append((pos[0], pos[1] if len(pos) > 1 else ""))
    if not slot_rows:  # fallback: pages that write bare 'Slot N: <effect>' lines, no template
        for mm in re.finditer(r"^\**\s*'*Slot\s+(\d+)\s*'*:\s*(.+?)\s*$",
                              f.get("slots", ""), re.I | re.M):
            slot_rows.append((mm.group(1), mm.group(2)))
    for slot_arg, body in slot_rows:
        if not slot_arg.isdigit():
            if "instrument" in slot_arg.lower():
                b = body.lower()
                instrument_flag = "REQUIRED" if "required" in b else \
                                  ("YES" if "yes" in b else instrument_flag)
            continue
        ef = parse_effect_text(body)
        ef["slot"] = int(slot_arg); ef["raw"] = re.sub(r"\s+", " ", body).strip()
        effects.append(ef)
        if ef.get("stack"):
            st = dict(ef["stack"]); st.update(src="WIKI_SLOT_ROW", verified=1, target_name=None)
            stacks.append(st)
    desc = f.get("description", "")
    dur_low = (f.get("duration") or "").strip().lower()
    is_buff = bool(beneficial) and dur_low not in ("", "instant")
    for rt, tn in parse_prose_stacking(desc, is_buff):
        stacks.append(dict(rule_type=rt, slot=None, opcode=None, cmp=None, value=None,
                           src="WIKI_PROSE", verified=0, target_name=tn))

    ops = {e["opcode"] for e in effects}
    illusion = next((e["illusion"] for e in effects if e["opcode"] == "ILLUSION" and e["illusion"]), None)
    is_illusion = 1 if ("Illusions" in cats or "ILLUSION" in ops) else 0
    dur = parse_spell_duration(f.get("duration", ""), is_song, beneficial, tbase)

    dec_slow = any(e["opcode"] in ("MOVE_SPEED", "HASTE") and (e["base"] or 0) < 0 for e in effects)
    dmg = any(e["opcode"] in ("HP", "MAX_HP", "HP_WHEN_CAST", "MANA") and (e["base"] or 0) < 0
              for e in effects)
    statful = any(e["opcode"] not in ("UNPARSED", "STACKING_RULE", "SUMMON_OTHER", "SUMMON_CORPSE",
                                      "ILLUSION", "UNKNOWN_STAT") and e["base"] is not None
                  for e in effects)
    if "SUMMON_PET" in ops: role = "PET_SUMMON"
    elif ops & {"ROOT", "MEZ", "CHARM", "FEAR", "STUN"} or (not beneficial and dec_slow): role = "CONTROL"
    elif not beneficial and dmg: role = "DAMAGE"
    elif beneficial and tbase == "PET": role = "PET_BUFF"
    elif beneficial and statful: role = "BUFF"
    else: role = "UTILITY"

    other = f.get("other", "")
    pet = None
    tok = next((e["pet_token"] for e in effects if e["opcode"] == "SUMMON_PET"), None)
    if "SUMMON_PET" in ops or re.search(r"Pet Level", other):
        oc = strip_links(re.sub(r"'''", "", other))
        def _oc(pattern):
            m = re.search(pattern, oc)
            return m.group(1).strip() if m else None
        pet = dict(token=tok, level=None, src=None,
                   classes=_oc(r"Pet Classes:?\s*([A-Z]{3}\s*/\s*[A-Z]{3})"),
                   hp=_oc(r"Pet Hit Points:?\s*([^\n*]+)"),
                   maxhit=_num(_oc(r"Pet Max Hit:?\s*(\d+)")),
                   ht=_num(_oc(r"Pet Harm Touch:?\s*(\d+)")),
                   lt=_num(_oc(r"Pet Lifetap:?\s*(\d+)")))
        lvl = _oc(r"Pet Level:?\s*(\d+)")
        if lvl: pet["level"], pet["src"] = int(lvl), "OTHER_BLOCK"
        elif tok:
            mt = re.match(r"skel_pet_(\d+)_", tok)
            if mt: pet["level"], pet["src"] = int(mt.group(1)), "TOKEN"

    return dict(
        pageid=pageid, name=name, name_canonical=canonical_name(name), page_title=title,
        icon=(f.get("spellicon") or "").strip() or None, skill=skill, skill_key=skill_key,
        mana=_num(f.get("mana")), range=_num(f.get("range")),
        cast=_fnum(f.get("casting_time")), fizzle=_fnum(f.get("fizzle_time")),
        recast=_fnum(f.get("recast_time")),
        duration_raw=(f.get("duration") or "").strip() or None, dur=dur,
        target_raw=target_raw, tbase=tbase, tmax=tmax, tonly=tonly,
        spell_type=spell_type, beneficial=beneficial,
        resist_type=resist_type, resist_adjust=resist_adjust,
        era=era, era_src=era_src, npc_only=npc_only,
        is_illusion=is_illusion, illusion=illusion, is_song=is_song, role=role,
        description=desc.strip() or None,
        msg_you=(f.get("msg_cast_on_you") or "").strip() or None,
        msg_other=(f.get("msg_cast_on_other") or "").strip() or None,
        msg_off=(f.get("msg_wears_off") or "").strip() or None,
        where_raw=(f.get("where_to_obtain") or "").strip() or None,
        other_raw=other.strip() or None, template=tname, raw=text, revid=revid,
        classes=classes, effects=effects, stacks=stacks, instrument_flag=instrument_flag,
        sources=parse_spell_sources(f.get("where_to_obtain", "")),
        item_sources=sorted({n.strip() for n in
                             re.findall(r"\{\{:([^}|]+?)\}\}", f.get("items_with_effect", ""))}),
        categories=cats, pet=pet)

# --------------------------------------------------------------------- spell load
SPELL_CHILDREN = ["spell_class_level", "spell_effect", "spell_target_rule", "spell_duration_rule",
                  "bard_song_rule", "spell_source", "spell_item_source", "spell_categories",
                  "spell_stacking_rule", "spell_pet_summon"]

def _bump(stats, key, n=1):
    if stats is not None: stats[key] = stats.get(key, 0) + n

def _delete_spell(con, sid):
    for t in SPELL_CHILDREN:
        con.execute("DELETE FROM %s WHERE spell_id=?" % t, (sid,))
    con.execute("DELETE FROM spell WHERE id=?", (sid,))

def load_spell(con, pid, title, text, revid=None, stats=None):
    sp = parse_spell(pid, title, text, revid)
    if not sp: return 0
    now = datetime.datetime.utcnow().isoformat(timespec="seconds")
    # wrong-spellname guard: some pages copy-paste another spell's |spellname= field
    # ('Circle of Butcherblock' says 'Ring of South Ro'). If the spellname collides with a
    # DIFFERENT page while our page title disagrees with it, trust the page title.
    title_canon = canonical_name(title)
    if sp["name_canonical"] != title_canon and con.execute(
            "SELECT 1 FROM spell WHERE name_canonical=? AND id!=?",
            (sp["name_canonical"], pid)).fetchone():
        sp["name"], sp["name_canonical"] = title, title_canon
        _bump(stats, "spellname_from_title")
    # case-variant duplicate titles: higher revid wins (plan §3.8)
    row = con.execute("SELECT id, source_revision, page_title FROM spell "
                      "WHERE name_canonical=? AND id!=?",
                      (sp["name_canonical"], pid)).fetchone()
    if row:
        ex_title_canon = canonical_name(row[2] or "")
        if row[2] and ex_title_canon != sp["name_canonical"]:
            # reverse order of the same wiki bug: the EXISTING row is the impostor —
            # rename it to its own page title, freeing the contested name
            try:
                con.execute("UPDATE spell SET name=?, name_canonical=? WHERE id=?",
                            (row[2], ex_title_canon, row[0]))
                _bump(stats, "spellname_from_title"); row = None
            except sqlite3.IntegrityError:
                pass  # its title is taken too -> fall through to the revid rule
    if row:
        if revid is not None and row[1] is not None and revid < row[1]:
            _bump(stats, "dup_skipped"); return 0
        _delete_spell(con, row[0]); _bump(stats, "dup_replaced")
    for t in SPELL_CHILDREN:  # child pre-delete: no stale rows on re-sync
        con.execute("DELETE FROM %s WHERE spell_id=?" % t, (pid,))
    con.execute("""INSERT OR REPLACE INTO spell(id,name,name_canonical,page_title,icon,casting_skill,
        mana,"range",casting_time,fizzle_time,recast_time,duration_raw,target_type_raw,spell_type_raw,
        is_beneficial,resist_type,resist_adjust,era,era_source,is_npc_only,is_illusion,
        illusion_race_id,is_song,role,description,msg_cast_on_you,msg_cast_on_other,msg_wears_off,
        where_to_obtain_raw,other_raw,template_name,raw_wikitext,source_revision,updated)
        VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
        (pid, sp["name"], sp["name_canonical"], sp["page_title"], sp["icon"], sp["skill"],
         sp["mana"], sp["range"], sp["cast"], sp["fizzle"], sp["recast"], sp["duration_raw"],
         sp["target_raw"], sp["spell_type"], sp["beneficial"], sp["resist_type"], sp["resist_adjust"],
         sp["era"], sp["era_src"], sp["npc_only"], sp["is_illusion"], sp["illusion"],
         sp["is_song"], sp["role"], sp["description"], sp["msg_you"], sp["msg_other"],
         sp["msg_off"], sp["where_raw"], sp["other_raw"], sp["template"], sp["raw"],
         revid, now))
    for abbr, lvl, auto in sp["classes"]:
        con.execute("INSERT OR REPLACE INTO spell_class_level VALUES(?,?,?,?,?)",
                    (pid, CLASS_ID[abbr], lvl, auto, revid))
        _bump(stats, "class_rows")
    inst_scaled = 1 if (sp["is_song"] and sp["instrument_flag"] in ("YES", "REQUIRED")) else 0
    for e in sp["effects"]:
        con.execute("""INSERT INTO spell_effect(spell_id,slot_number,raw_text,opcode,stat,
            base_amount,max_amount,min_caster_level,max_caster_level,caster_level_scaling,
            is_percent,resource_mode,per_tick_increment,tier_scaling_json,instrument_scaled,
            is_cosmetic,grants_proc_spell_id,pet_token,is_stacking_rule,verification_status,
            source_revision) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,NULL,?,?,NULL,?,?,?,?)""",
            (pid, e["slot"], e["raw"], e["opcode"], e["stat"], e["base"], e["max"],
             e["minlvl"], e["maxlvl"], e["scaling"], e["pct"], e["resource"], e["ptinc"],
             inst_scaled if e["opcode"] not in ("STACKING_RULE",) else 0,
             e["cosmetic"], e["pet_token"], 1 if e["opcode"] == "STACKING_RULE" else 0,
             "WIKI_CONFIRMED", revid))
        _bump(stats, "effects")
        if e["opcode"] == "UNPARSED": _bump(stats, "effect_unparsed")
        if e["opcode"] == "UNKNOWN_STAT": _bump(stats, "effect_unknown_stat")
    pt = 1 if (sp["tbase"] in ("SINGLE", "GROUP") and sp["beneficial"]) else \
         (0 if sp["tbase"] == "SELF" else (1 if sp["tbase"] == "PET" else None))
    only_json = json.dumps({"only_body_type": sp["tonly"]}) if sp.get("tonly") else None
    con.execute("""INSERT OR REPLACE INTO spell_target_rule(spell_id,target_base,pet_targetable,
        pet_targetable_status,pet_subtype,target_level_min,target_level_max,
        excluded_target_types_json,verification_status,source_revision)
        VALUES(?,?,?,'NEEDS_INGAME_TEST','ANY',NULL,?,?,'WIKI_CONFIRMED',?)""",
        (pid, sp["tbase"], pt, sp["tmax"], only_json, revid))
    if sp["tbase"] == "UNKNOWN": _bump(stats, "unknown_target")
    d = sp["dur"]
    con.execute("""INSERT OR REPLACE INTO spell_duration_rule(spell_id,duration_class,
        maintenance_type,duration_seconds_min,duration_seconds_max,duration_min_caster_level,
        duration_max_caster_level,tick_count,recast_time,verification_status,source_revision)
        VALUES(?,?,?,?,?,?,?,?,?,'WIKI_CONFIRMED',?)""",
        (pid, d["cls"], d["maint"], d["smin"], d["smax"], d["lmin"], d["lmax"], d["ticks"],
         sp["recast"], revid))
    if d["cls"] == "UNKNOWN" and sp["duration_raw"]: _bump(stats, "unknown_duration")
    if sp["is_song"]:
        con.execute("""INSERT OR REPLACE INTO bard_song_rule(spell_id,cast_time,duration_ticks,
            instrument_type,instrument_scaling_allowed,is_sustainable,minimum_cycle_time,
            bard_layer,verification_status,source_revision)
            VALUES(?,?,?,?,?,1,NULL,NULL,'PARTIALLY_VERIFIED',?)""",
            (pid, sp["cast"], d["ticks"],
             INSTRUMENT_SKILLS.get(sp["skill_key"], "NONE"),
             sp["instrument_flag"] or "NO", revid))
        _bump(stats, "songs")
    for s_ in sp["sources"]:
        con.execute("""INSERT INTO spell_source(spell_id,source_type,zone_name,npc_name,area,loc,
            raw_text,source_revision) VALUES(?,?,?,?,?,?,?,?)""",
            (pid, s_["type"], s_["zone"], s_["npc"], s_["area"], s_["loc"], s_["raw"], revid))
    for iname in sp["item_sources"]:
        con.execute("INSERT OR REPLACE INTO spell_item_source VALUES(?,?)", (pid, iname))
    for c in sp["categories"]:
        con.execute("INSERT OR REPLACE INTO spell_categories VALUES(?,?)", (pid, c))
    for st in sp["stacks"]:
        aff = None
        if st["target_name"]:
            r = con.execute("SELECT id FROM spell WHERE name_canonical=?",
                            (canonical_name(st["target_name"]),)).fetchone()
            aff = r[0] if r else None
        con.execute("""INSERT INTO spell_stacking_rule(spell_id,rule_type,affected_spell_id,
            affected_buff_line_id,affected_effect_slot,affected_effect_opcode,comparison_operator,
            comparison_value,priority,order_dependent,source_type,verified,source_revision,notes)
            VALUES(?,?,?,NULL,?,?,?,?,0,0,?,?,?,?)""",
            (pid, st["rule_type"], aff, st["slot"], st["opcode"], st["cmp"], st["value"],
             st["src"], st["verified"], revid,
             ("target=" + st["target_name"]) if (st["target_name"] and aff is None) else None))
        _bump(stats, "stacking_rules")
        if st["src"] == "WIKI_PROSE": _bump(stats, "prose_rules")
    if sp["pet"]:
        p = sp["pet"]
        con.execute("""INSERT OR REPLACE INTO spell_pet_summon(spell_id,pet_archetype_id,
            pet_classes,summon_token,base_pet_level,base_level_source,base_pet_level_status,
            pet_hp,pet_hp_numeric,pet_max_hit,pet_harm_touch,pet_lifetap,source_revision)
            VALUES(?,NULL,?,?,?,?,?,?,?,?,?,?,?)""",
            (pid, p["classes"], p["token"], p["level"], p["src"],
             "WIKI_CONFIRMED" if p["level"] is not None else "NEEDS_INGAME_TEST",
             p["hp"], _num(p["hp"]), p["maxhit"], p["ht"], p["lt"], revid))
        _bump(stats, "pet_summons")
        if p["level"] is None: _bump(stats, "pet_level_unknown")
    _bump(stats, "spells")
    return 1

def load_pet_page(con, pid, title, text, revid=None, stats=None):
    f = parse_template_fields(text, "Summonedpetpage")
    if f is None: return 0
    g = lambda k: (f.get(k) or "").strip()
    con.execute("DELETE FROM pet_stat_block WHERE page_pageid=?", (pid,))
    con.execute("""INSERT INTO pet_stat_block(page_pageid,summon_spell_id,summoning_spell_name,
        pet_archetype_id,pet_classes,level,hp,hp_regen,mana,mana_regen,mitigation,avoidance,
        offense,accuracy,str,sta,agi,dex,wis,intel,cha,max_damage,dual_wields,abilities,
        innate_spells_raw,verification_status,source_revision)
        VALUES(?,NULL,?,NULL,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,'WIKI_CONFIRMED',?)""",
        (pid, strip_links(g("summoning_spell")).strip() or None, g("classes") or None,
         _num(g("level")), _num(g("hp")), _num(g("hp_regen")), _num(g("mana")),
         _num(g("mana_regen")), _num(g("mitigation")), _num(g("avoidance")), _num(g("offense")),
         _num(g("accuracy")), _num(g("strength")), _num(g("stamina")), _num(g("agility")),
         _num(g("dexterity")), _num(g("wisdom")), _num(g("intelligence")), _num(g("charisma")),
         _num(g("max_damage")), g("dual_wields") or None,
         strip_links(g("abilities")).strip() or None,
         strip_links(g("spells")).strip() or None, revid))
    _bump(stats, "petpages")
    return 1

def spell_lookup_map(con):
    """canonical name -> spell id, matching BOTH the spellname and the page TITLE.
    Wiki links target titles, so title matching fixes spellname-typo pages (e.g. page
    'Maniacal Strength' whose spellname field says 'Manicial Strength'). Registered
    titles first so a name entry wins any collision."""
    look = {}
    for sid, title in con.execute("SELECT id, page_title FROM spell WHERE page_title IS NOT NULL"):
        look[canonical_name(title)] = sid
    for sid, nc in con.execute("SELECT id, name_canonical FROM spell"):
        look[nc] = sid
    return look

def finalize_spells(con):
    """Post-pass: resolve name-based links that could not resolve during load order."""
    fixed = 0
    look = spell_lookup_map(con)
    for rid, note in con.execute("SELECT id, notes FROM spell_stacking_rule "
                                 "WHERE affected_spell_id IS NULL AND notes LIKE 'target=%'").fetchall():
        sid = look.get(canonical_name(note[7:]))
        if sid:
            con.execute("UPDATE spell_stacking_rule SET affected_spell_id=?, notes=NULL WHERE id=?",
                        (sid, rid)); fixed += 1
    for ppid, nm in con.execute("SELECT page_pageid, summoning_spell_name FROM pet_stat_block "
                                "WHERE summon_spell_id IS NULL AND summoning_spell_name IS NOT NULL").fetchall():
        sid = look.get(canonical_name(nm))
        if sid:
            con.execute("UPDATE pet_stat_block SET summon_spell_id=? WHERE page_pageid=?",
                        (sid, ppid)); fixed += 1
    for eid, nm in con.execute("SELECT id, stat FROM spell_effect WHERE opcode='ADD_PROC' "
                               "AND grants_proc_spell_id IS NULL AND stat IS NOT NULL").fetchall():
        sid = look.get(canonical_name(nm))
        if sid:
            con.execute("UPDATE spell_effect SET grants_proc_spell_id=? WHERE id=?",
                        (sid, eid)); fixed += 1
    con.commit()
    return fixed

# ---------------------------------------------------------------------------- load
def load_pages(con, pages, stats=None):
    """Total dispatch: rows are (pid, title, text) or (pid, title, text, revid)."""
    now = datetime.datetime.utcnow().isoformat(timespec="seconds")
    ni = nm = 0
    for row in pages:
        pid, title, text = row[0], row[1], row[2]
        revid = row[3] if len(row) > 3 else None
        if "{{Summonedpetpage" in text:
            load_pet_page(con, pid, title, text, revid, stats); continue
        if "{{Spellpage" in text:  # matches Spellpagesmart too
            load_spell(con, pid, title, text, revid, stats); continue
        if "{{Namedmobpage" in text:
            m = parse_mob(pid, title, text)
            if not m: continue
            con.execute("DELETE FROM mobs WHERE name=? AND pageid!=?", (m["name"], m["pageid"]))
            con.execute("""INSERT INTO mobs(pageid,name,race,class,level,zone,loc,respawn,hp,
               dmg_per_hit,attacks_per_round,attack_speed,special,era,updated)
               VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
               ON CONFLICT(pageid) DO UPDATE SET name=excluded.name,race=excluded.race,
               class=excluded.class,level=excluded.level,zone=excluded.zone,loc=excluded.loc,
               respawn=excluded.respawn,hp=excluded.hp,dmg_per_hit=excluded.dmg_per_hit,
               attacks_per_round=excluded.attacks_per_round,attack_speed=excluded.attack_speed,
               special=excluded.special,era=excluded.era,updated=excluded.updated""",
               (m["pageid"],m["name"],m["race"],m["class"],m["level"],m["zone"],m["loc"],
                m["respawn"],m["hp"],m["dmg_per_hit"],m["attacks_per_round"],m["attack_speed"],
                m["special"],m["era"],now))
            for iname, rar in m["loot"]:
                con.execute("INSERT OR REPLACE INTO drops(mob_name,item_name,rarity) VALUES(?,?,?)",
                            (m["name"], iname, rar))
            nm += 1
        elif "{{Itempage" in text:
            it = parse_item(pid, title, text)
            if not it: continue
            con.execute("DELETE FROM items WHERE name=? AND pageid!=?", (it["name"], it["pageid"]))
            con.execute("""INSERT INTO items(pageid,name,icon_id,slot,weapon_skill,atk_delay,dmg,ac,
               haste_pct,worn_effect,focus_effect,click_effect,flags,era,notes,merchant_value,
               raw_statsblock,updated) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
               ON CONFLICT(pageid) DO UPDATE SET name=excluded.name,icon_id=excluded.icon_id,
               slot=excluded.slot,weapon_skill=excluded.weapon_skill,atk_delay=excluded.atk_delay,
               dmg=excluded.dmg,ac=excluded.ac,haste_pct=excluded.haste_pct,
               worn_effect=excluded.worn_effect,focus_effect=excluded.focus_effect,
               click_effect=excluded.click_effect,flags=excluded.flags,era=excluded.era,
               notes=excluded.notes,merchant_value=excluded.merchant_value,
               raw_statsblock=excluded.raw_statsblock,updated=excluded.updated""",
               (it["pageid"],it["name"],it["icon_id"],it["slot"],it["weapon_skill"],it["atk_delay"],
                it["dmg"],it["ac"],it["haste_pct"],it["worn_effect"],it["focus_effect"],
                it["click_effect"],it["flags"],it["era"],it["notes"],it["merchant_value"],
                it["raw_statsblock"],now))
            for k, v in it["stats"].items():
                con.execute("INSERT OR REPLACE INTO item_stats VALUES(?,?,?)",(it["pageid"],k,v))
            for c in it["classes"]:
                con.execute("INSERT OR REPLACE INTO item_classes VALUES(?,?)",(it["pageid"],c))
            for r in it["races"]:
                con.execute("INSERT OR REPLACE INTO item_races VALUES(?,?)",(it["pageid"],r))
            for c in it["categories"]:
                con.execute("INSERT OR REPLACE INTO item_categories VALUES(?,?)",(it["pageid"],c))
            ni += 1
    con.commit()
    return ni, nm

# --------------------------------------------------------------------------- fetch
def _session():
    import requests
    s = requests.Session(); s.headers["User-Agent"] = UA
    return s

def api_get(s, params):
    params = dict(params); params.setdefault("format","json")
    for attempt in range(5):
        r = s.get(API, params=params, timeout=60)
        if r.status_code == 200: return r.json()
        time.sleep(2 * (attempt+1))
    r.raise_for_status()

def fetch_category(s, category, save_raw=True):
    """Yield (pageid, title, wikitext) for all members of a category, following continuation."""
    cont = {}; batch = []
    while True:
        p = {"action":"query","generator":"categorymembers","gcmtitle":f"Category:{category}",
             "gcmlimit":"50","prop":"revisions","rvprop":"content","rvslots":"main"}
        p.update(cont)
        data = api_get(s, p)
        pages = data.get("query",{}).get("pages",{})
        for pid, pg in pages.items():
            try:
                txt = pg["revisions"][0]["slots"]["main"]["*"]
            except (KeyError, IndexError):
                continue
            yield int(pg["pageid"]), pg["title"], txt
            batch.append({"pageid":pg["pageid"],"title":pg["title"],"wikitext":txt})
        if "continue" in data:
            cont = data["continue"]; time.sleep(0.5)
        else:
            break
    if save_raw and batch:
        os.makedirs(RAW, exist_ok=True)
        safe = re.sub(r"[^A-Za-z0-9]+","_",category).strip("_").lower()
        with open(os.path.join(RAW, f"{safe}.json"),"w",encoding="utf-8") as fh:
            json.dump(batch, fh, ensure_ascii=False, indent=1)

def fetch_category_rev(s, category, save_raw=True):
    """Like fetch_category but also yields revid (for spell provenance + duplicate dedup)."""
    cont = {}; batch = []
    while True:
        p = {"action":"query","generator":"categorymembers","gcmtitle":f"Category:{category}",
             "gcmlimit":"50","prop":"revisions","rvprop":"content|ids","rvslots":"main"}
        p.update(cont)
        data = api_get(s, p)
        for pid, pg in data.get("query",{}).get("pages",{}).items():
            try:
                rev = pg["revisions"][0]; txt = rev["slots"]["main"]["*"]
            except (KeyError, IndexError):
                continue
            revid = rev.get("revid")
            yield int(pg["pageid"]), pg["title"], txt, revid
            batch.append({"pageid":pg["pageid"],"title":pg["title"],"revid":revid,"wikitext":txt})
        if "continue" in data:
            cont = data["continue"]; time.sleep(0.5)
        else:
            break
    if save_raw and batch:
        os.makedirs(RAW, exist_ok=True)
        safe = re.sub(r"[^A-Za-z0-9]+","_",category).strip("_").lower()
        with open(os.path.join(RAW, f"{safe}.json"),"w",encoding="utf-8") as fh:
            json.dump(batch, fh, ensure_ascii=False, indent=1)

def fetch_incremental(s, since_iso):
    """Yield pages edited since `since_iso` (ISO ts). Uses recentchanges."""
    cont = {}
    while True:
        p = {"action":"query","list":"recentchanges","rcend":since_iso,"rclimit":"100",
             "rcprop":"title|ids","rcnamespace":"0","rctype":"edit|new"}
        p.update(cont)
        data = api_get(s, p)
        titles = [rc["title"] for rc in data.get("query",{}).get("recentchanges",[])]
        for i in range(0, len(titles), 40):
            chunk = titles[i:i+40]
            d2 = api_get(s, {"action":"query","titles":"|".join(chunk),
                             "prop":"revisions","rvprop":"content","rvslots":"main"})
            for pid, pg in d2.get("query",{}).get("pages",{}).items():
                try: txt = pg["revisions"][0]["slots"]["main"]["*"]
                except (KeyError, IndexError): continue
                yield int(pg["pageid"]), pg["title"], txt
        if "continue" in data: cont = data["continue"]; time.sleep(0.5)
        else: break

# ------------------------------------------------------------------------ commands
def cmd_sync(args):
    s = _session(); con = db()
    cats = []
    if args.category: cats = [args.category]
    else:
        if args.gear or not (args.gear or args.mobs): cats += GEAR_CATEGORIES
        if args.mobs or not (args.gear or args.mobs): cats += MOB_CATEGORIES
    if args.incremental:
        last = con.execute("SELECT value FROM sync_meta WHERE key='last_sync'").fetchone()
        since = last[0] if last else (datetime.datetime.utcnow()-datetime.timedelta(days=7)).isoformat()
        print(f"Incremental since {since}")
        ni, nm = load_pages(con, list(fetch_incremental(s, since)))
        print(f"  updated items={ni} mobs={nm}")
    else:
        for c in cats:
            print(f"Fetching category: {c} ...")
            ni, nm = load_pages(con, list(fetch_category(s, c)))
            print(f"  items={ni} mobs={nm}")
    con.execute("INSERT OR REPLACE INTO sync_meta VALUES('last_sync',?)",
                (datetime.datetime.utcnow().isoformat(timespec='seconds'),))
    con.commit(); print("Done. DB:", DB)

def cmd_sync_spells(args):
    """M0 spell importer: pull all spell pages (+ BST warder pages) into the spell tables."""
    s = _session(); con = db(); stats = {}
    cats = [args.category] if args.category else \
           ["Spells", "NPC Only Spells", "Summoned Pet", "Beastlord Pet"]
    for c in cats:
        print(f"Fetching category: {c} ...")
        n, npc_ids = 0, []
        for pid, title, txt, revid in fetch_category_rev(s, c):
            if "{{Summonedpetpage" in txt:
                n += load_pet_page(con, pid, title, txt, revid, stats)
            elif "{{Spellpage" in txt:
                k = load_spell(con, pid, title, txt, revid, stats)
                n += k
                if k and c == "NPC Only Spells": npc_ids.append(pid)
            else:
                stats["skipped_pages"] = stats.get("skipped_pages", 0) + 1
        if npc_ids:
            con.executemany("UPDATE spell SET is_npc_only=1 WHERE id=?", [(i,) for i in npc_ids])
        con.commit()
        print(f"  loaded={n}")
    fixed = finalize_spells(con)
    con.execute("INSERT OR REPLACE INTO sync_meta VALUES('last_spell_sync',?)",
                (datetime.datetime.utcnow().isoformat(timespec='seconds'),))
    con.commit()
    # ---- run report (M0 definition-of-done gates)
    q = lambda sql: con.execute(sql).fetchone()[0]
    total   = q("SELECT COUNT(*) FROM spell")
    effects = q("SELECT COUNT(*) FROM spell_effect WHERE is_stacking_rule=0")
    unparsed = q("SELECT COUNT(*) FROM spell_effect WHERE opcode='UNPARSED'")
    print("\n=== sync-spells report ===")
    print(f"spells={total}  class_rows={q('SELECT COUNT(*) FROM spell_class_level')}"
          f"  effects={effects}  stacking_rules={q('SELECT COUNT(*) FROM spell_stacking_rule')}"
          f"  (prose={stats.get('prose_rules',0)}, resolved_links={fixed})")
    print(f"songs={q('SELECT COUNT(*) FROM bard_song_rule')}"
          f"  pet_summons={q('SELECT COUNT(*) FROM spell_pet_summon')}"
          f"  (level_unknown={stats.get('pet_level_unknown',0)})"
          f"  warder_pages={q('SELECT COUNT(*) FROM pet_stat_block')}"
          f"  npc_only={q('SELECT COUNT(*) FROM spell WHERE is_npc_only=1')}")
    pct = (100.0 * unparsed / effects) if effects else 0.0
    print(f"effect rows UNPARSED={unparsed} ({pct:.1f}%)  UNKNOWN_STAT={stats.get('effect_unknown_stat',0)}"
          f"  unknown_target={stats.get('unknown_target',0)}"
          f"  unknown_duration={stats.get('unknown_duration',0)}")
    print(f"duplicate titles: replaced={stats.get('dup_replaced',0)} skipped={stats.get('dup_skipped',0)}"
          f"  non-template pages skipped={stats.get('skipped_pages',0)}")
    if pct >= 2.0:
        print("WARNING: unparsed-effect rate >= 2% (M0 gate) - inspect spell_effect WHERE opcode='UNPARSED'")
    print("Done. DB:", DB)

def cmd_load(args):
    con = db()
    with open(args.from_raw, encoding="utf-8") as fh: data = json.load(fh)
    # accept either our raw list or a MediaWiki api response
    pages = []
    if isinstance(data, dict) and "query" in data:
        for pid, pg in data["query"]["pages"].items():
            pages.append((int(pg["pageid"]), pg["title"], pg["revisions"][0]["slots"]["main"]["*"]))
    else:
        for row in data:
            pages.append((int(row["pageid"]), row["title"], row["wikitext"], row.get("revid")))
    stats = {}
    ni, nm = load_pages(con, pages, stats)
    finalize_spells(con)
    print(f"Loaded items={ni} mobs={nm} spells={stats.get('spells',0)} "
          f"petpages={stats.get('petpages',0)} from {args.from_raw}")

def cmd_export(args):
    con = db(); con.row_factory = sqlite3.Row; os.makedirs(EXP, exist_ok=True)
    items = [dict(r) for r in con.execute("SELECT * FROM items ORDER BY name")]
    for it in items:
        pid = it["pageid"]
        it["stats"]   = {r["stat"]:r["value"] for r in con.execute("SELECT stat,value FROM item_stats WHERE pageid=?",(pid,))}
        it["classes"] = [r[0] for r in con.execute("SELECT class FROM item_classes WHERE pageid=?",(pid,))]
        it["races"]   = [r[0] for r in con.execute("SELECT race FROM item_races WHERE pageid=?",(pid,))]
    json.dump(items, open(os.path.join(EXP,"items.json"),"w",encoding="utf-8"), ensure_ascii=False, indent=1)
    mobs = [dict(r) for r in con.execute("SELECT * FROM mobs ORDER BY name")]
    json.dump(mobs, open(os.path.join(EXP,"mobs.json"),"w",encoding="utf-8"), ensure_ascii=False, indent=1)
    spells = [dict(r) for r in con.execute(
        """SELECT id,name,icon,casting_skill,mana,"range",casting_time,recast_time,duration_raw,
           target_type_raw,spell_type_raw,is_beneficial,resist_type,resist_adjust,era,
           is_npc_only,is_illusion,is_song,role FROM spell ORDER BY name""")]
    for sp in spells:
        sid = sp["id"]
        sp["classes"] = {r["abbr"]: {"level": r["required_class_level"],
                                     "autogranted": bool(r["is_autogranted"])}
                         for r in con.execute(
                             "SELECT c.abbr, l.required_class_level, l.is_autogranted "
                             "FROM spell_class_level l JOIN class c ON c.id=l.class_id "
                             "WHERE l.spell_id=?", (sid,))}
        sp["effects"] = [dict(r) for r in con.execute(
            "SELECT slot_number,raw_text,opcode,stat,base_amount,max_amount,min_caster_level,"
            "max_caster_level,is_percent,resource_mode,pet_token FROM spell_effect "
            "WHERE spell_id=? AND is_stacking_rule=0 ORDER BY slot_number", (sid,))]
    json.dump(spells, open(os.path.join(EXP,"spells.json"),"w",encoding="utf-8"),
              ensure_ascii=False, indent=1)
    print(f"Exported {len(items)} items, {len(mobs)} mobs, {len(spells)} spells to {EXP}")

def cmd_theorycraft(args):
    """Gear wearable by a multiclass character + the worn/focus mods you could harvest."""
    classes = [c.upper() for c in args.classes]
    con = db(); con.row_factory = sqlite3.Row
    ph = ",".join("?"*len(classes))
    # An item is wearable if any of its classes is in the build, OR it lists ALL.
    wearable = con.execute(f"""
        SELECT DISTINCT i.name, i.slot, i.era, i.haste_pct, i.ac, i.dmg,
               i.worn_effect, i.focus_effect, i.click_effect
        FROM items i LEFT JOIN item_classes c ON c.pageid=i.pageid
        WHERE c.class IN ({ph}) OR i.raw_statsblock LIKE '%Class: ALL%'
        ORDER BY i.slot, i.name""", classes).fetchall()
    print(f"\n=== Gear wearable by {'/'.join(classes)} : {len(wearable)} items ===")
    for r in wearable[:args.limit]:
        mods = [m for m in (r["worn_effect"],r["focus_effect"],r["click_effect"]) if m]
        print(f"  [{r['slot'] or '?':<18}] {r['name']:<32} {r['era'] or '':<12}"
              + (f"  <{', '.join(mods)}>" if mods else ""))
    # Transferable mods: distinct worn/focus effects available on wearable gear
    print(f"\n=== Transferable worn/focus mods available to this build ===")
    seen = {}
    for r in wearable:
        pairs = [("worn",r["worn_effect"]),("focus",r["focus_effect"]),("click",r["click_effect"])]
        if r["haste_pct"]: pairs.append(("haste", f"+{r['haste_pct']}% Worn Haste"))
        for kind, val in pairs:
            if val: seen.setdefault((kind,val), []).append(r["name"])
    for (kind,val), srcs in sorted(seen.items()):
        print(f"  {kind:5} | {val:<34} from: {', '.join(srcs[:3])}" + (" ..." if len(srcs)>3 else ""))

def cmd_map_categories(args):
    s = _session(); out = {}; cont = {}
    while True:
        p = {"action":"query","list":"allcategories","acprop":"size","aclimit":"500"}; p.update(cont)
        d = api_get(s, p)
        for c in d["query"]["allcategories"]:
            out[c["*"]] = {"size":c["size"],"pages":c["pages"],"subcats":c["subcats"]}
        if "continue" in d: cont = d["continue"]
        else: break
    json.dump(out, open(os.path.join(BASE,"categories_full.json"),"w",encoding="utf-8"), indent=1)
    print(f"Wrote categories_full.json ({len(out)} categories)")

def main():
    ap = argparse.ArgumentParser(description="EQ Legends wiki -> local SQLite mirror")
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("sync"); a.add_argument("--gear",action="store_true")
    a.add_argument("--mobs",action="store_true"); a.add_argument("--category")
    a.add_argument("--incremental",action="store_true"); a.set_defaults(func=cmd_sync)
    a = sub.add_parser("sync-spells"); a.add_argument("--category")
    a.set_defaults(func=cmd_sync_spells)
    a = sub.add_parser("load"); a.add_argument("--from-raw",required=True); a.set_defaults(func=cmd_load)
    a = sub.add_parser("export"); a.set_defaults(func=cmd_export)
    a = sub.add_parser("map-categories"); a.set_defaults(func=cmd_map_categories)
    a = sub.add_parser("theorycraft"); a.add_argument("classes",nargs="+")
    a.add_argument("--limit",type=int,default=40); a.set_defaults(func=cmd_theorycraft)
    args = ap.parse_args(); args.func(args)


if __name__ == "__main__":
    main()
