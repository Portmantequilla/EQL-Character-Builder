-- builds.db — the PRECIOUS database (plan §2.2). Lives in %LOCALAPPDATA%/EQLBuilder,
-- never a cloud-synced folder. App migration owns this DDL. Build rows use SOFT references
-- (pageid + name_canonical) to wiki.db and are re-resolved on load (§2.2.0).
-- This scaffold ships the core M1 tables; the full set is plan §2.2 / generated schema.sql.
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- ---- versioning + provenance ----
CREATE TABLE IF NOT EXISTS data_version (
  id INTEGER PRIMARY KEY, label TEXT NOT NULL UNIQUE,
  wiki_last_sync TEXT, notes TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS formula_version (
  id INTEGER PRIMARY KEY, label TEXT NOT NULL UNIQUE,
  notes TEXT, created_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS app_meta (key TEXT PRIMARY KEY, value TEXT);

-- ---- the one editable formula store (plan §2.2 Group 2) ----
CREATE TABLE IF NOT EXISTS formula_table (
  formula_key TEXT NOT NULL, dim1 TEXT NOT NULL DEFAULT '', dim2 TEXT NOT NULL DEFAULT '',
  dim3 TEXT NOT NULL DEFAULT '', value_int INTEGER, value_text TEXT, description TEXT,
  is_user_edited INTEGER NOT NULL DEFAULT 0,
  verification_status TEXT NOT NULL CHECK (verification_status IN
    ('WIKI_CONFIRMED','PARTIALLY_VERIFIED','NEEDS_INGAME_TEST','MANUAL_OVERRIDE','LEGACY_EQ_DATA','VERIFIED_INGAME')),
  formula_version_id INTEGER REFERENCES formula_version(id), source TEXT,
  PRIMARY KEY (formula_key, dim1, dim2, dim3));

-- ---- pet equipment rule (versioned; highest id wins) ----
CREATE TABLE IF NOT EXISTS pet_equipment_rule (
  id INTEGER PRIMARY KEY, rule_json TEXT NOT NULL, notes TEXT,
  created_at TEXT NOT NULL DEFAULT (datetime('now')));

-- ---- builds (soft refs to wiki.db) ----
CREATE TABLE IF NOT EXISTS build (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL, race TEXT, deity TEXT, level INTEGER,
  data_version_id INTEGER REFERENCES data_version(id),
  created_at TEXT NOT NULL DEFAULT (datetime('now')),
  updated_at TEXT NOT NULL DEFAULT (datetime('now')));
CREATE TABLE IF NOT EXISTS build_class (
  build_id INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  slot INTEGER NOT NULL CHECK (slot IN (1,2,3)), class TEXT NOT NULL,
  PRIMARY KEY (build_id, slot));
CREATE TABLE IF NOT EXISTS build_equipment (
  build_id INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  slot TEXT NOT NULL,                       -- paperdoll slot incl. BANDOLIER
  item_pageid INTEGER, item_name_canonical TEXT,   -- soft ref
  upgrade_tier INTEGER NOT NULL DEFAULT 0 CHECK (upgrade_tier BETWEEN 0 AND 10),
  status TEXT NOT NULL DEFAULT 'ACTIVE'
    CHECK (status IN ('ACTIVE','SAVED_INACTIVE','DATA_MISSING')),
  PRIMARY KEY (build_id, slot));
CREATE TABLE IF NOT EXISTS build_spell_tier (
  build_id INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  spell_pageid INTEGER, spell_name_canonical TEXT,
  spell_upgrade_tier INTEGER NOT NULL DEFAULT 0 CHECK (spell_upgrade_tier BETWEEN 0 AND 10),
  PRIMARY KEY (build_id, spell_name_canonical));
CREATE TABLE IF NOT EXISTS build_wishlist (
  build_id INTEGER NOT NULL REFERENCES build(id) ON DELETE CASCADE,
  item_pageid INTEGER, item_name_canonical TEXT, note TEXT,
  PRIMARY KEY (build_id, item_name_canonical));

-- Loot Filter catalog: real GAME item ids (tier-independent) harvested from the user's own
-- LF_*.ini filters and *-Inventory.txt dumps. The game matches loot on this id, which is NOT
-- our wiki pageid and lives in no bulk client file, so this table is the only place the loot
-- filter picker can offer ids the game will actually honor. Keyed by game id; pageid is the
-- wiki match (nullable) for stat/tooltip display.
CREATE TABLE IF NOT EXISTS known_game_item (
  game_item_id INTEGER PRIMARY KEY,
  name         TEXT NOT NULL,          -- base name, "+N" tier stripped
  name_key     TEXT NOT NULL,          -- normalized (norm()) for substring search / bridge
  icon_id      INTEGER,
  pageid       INTEGER,                -- wiki pageid matched by name, if any
  source       TEXT,                   -- 'lf' | 'inventory'
  updated      TEXT);
CREATE INDEX IF NOT EXISTS idx_known_game_item_namekey ON known_game_item(name_key);

-- ---- shipped seeds (M1 DoD: fv1, dv1, formula defaults, pet rule row 1) ----
INSERT OR IGNORE INTO formula_version(id,label,notes) VALUES (1,'fv1','shipped seed defaults');
INSERT OR IGNORE INTO data_version(id,label,notes) VALUES (1,'dv1','seeded wiki.db copy');
INSERT OR IGNORE INTO app_meta(key,value) VALUES
  ('active_data_version','1'), ('active_formula_version','1'), ('builds_schema_version','1');
INSERT OR IGNORE INTO pet_equipment_rule(id,rule_json,notes) VALUES
  (1,'{"slots_base":4,"bonus":{"MAG":3,"BST":3,"NEC":2,"ENC":1,"DRU":1,"SHM":1,"SHD":0},"usable_classes":"pet_classes UNION owner_classes"}','plan §14 / Pet Guide');
INSERT OR IGNORE INTO formula_table(formula_key,value_text,description,verification_status,formula_version_id,source) VALUES
  ('class_attr_combine','SUM','multiclass attribute combine: race base + each class''s +30 points, additive','PARTIALLY_VERIFIED',1,'eqltools.com Attributes (client-mined: each class contributes exactly 30)'),
  ('multi_class_hp_combine','TOP2_SUM','HP from the TWO highest classes summed; the third is dropped','PARTIALLY_VERIFIED',1,'eqltools.com Attributes (client-mined; also applies to endurance)'),
  ('multi_class_mana_combine','TOP2_SUM','mana from the TWO highest classes summed; a third caster adds NO mana','PARTIALLY_VERIFIED',1,'eqltools.com Attributes (client-mined)'),
  ('stat_naked_ceiling','150','naked (unbuffed, ungeared) per-attribute ceiling players report; not client-enforced','NEEDS_INGAME_TEST',1,'eqltools.com Attributes (player-reported)'),
  ('multi_class_skill_combine','BEST_OF','multiclass skill combine','NEEDS_INGAME_TEST',1,'plan §4.7 gap 7'),
  ('spell_tier_scaling','','+10%/tier, round down, min +1 (mirrors item rule)','NEEDS_INGAME_TEST',1,'plan §4.7 gap 6'),
  ('stat_cap','510','buffed attribute hard cap (STR..CHA)','PARTIALLY_VERIFIED',1,'EQL Discord community-reported 2026-07-21'),
  ('resist_cap','1000','buffed resist/save hard cap (SV MAGIC..DISEASE)','PARTIALLY_VERIFIED',1,'EQL Discord community-reported 2026-07-21'),
  ('item_tier_scaling_pct','10','DEPRECATED for items (2026-07-23): the exact community rule is hardcoded (<=10 +1/tier; >10 & dmg INT(B+ROUND(B*T)/10); haste +1/tier; negatives MIN(0,B+T))','VERIFIED_INGAME',1,'Mosscovered Legend''s EQL Stat Estimator Item Estimator sheet (100% parity; Keg Mallet confirmed)'),
  ('spell_tier_scaling_pct','6','spell damage/healing: +6% of base per tier, LINEAR, floor (also scales buff values in this planner — stat-buff scaling itself is unverified)','PARTIALLY_VERIFIED',1,'community-reconstructed 2026-07 (Ice Comet 808 -> ~1050 @T5); official 7/7 notes confirm dmg+heal scale but not the %'),
  ('spell_tier_mana_pct','6','spell mana cost: ~-6% of base per tier, rounded — PROVISIONAL (min-mana floors + rounding; Minor Healing stays 10 at T2)','NEEDS_INGAME_TEST',1,'player reports ~60% cheaper at T10; needs per-spell verification'),
  ('spell_tier_mana_floor','20','base mana below this never shows tier reduction (observed: 10-mana Minor Healing unchanged at T2)','NEEDS_INGAME_TEST',1,'wiki Minor Healing T2 screenshot'),
  ('spell_tier_cast_pct','4','cast/recovery/reuse time: -4% of base per tier (wiki: 1.50s -> 1.38s at T2)','PARTIALLY_VERIFIED',1,'wiki Spell_Upgrading guide + Minor-healing-2 screenshot'),
  ('reagent_conserve_pct_per_tier','10','chance per tier to conserve reagents (100% at T10)','WIKI_CONFIRMED',1,'official 7/7/2026 patch notes'),
  ('buff_slot_cap','30','max simultaneous buffs (wiki says 15 incl. songs, but in-game is higher — count and set here)','NEEDS_INGAME_TEST',1,'wiki Buff Lines page contradicted by live game'),
  ('class_2_unlock_level','1','level at which the SECOND class unlocks (contributes spells)','NEEDS_INGAME_TEST',1,'unknown — user reports the third unlocks at 11; second assumed from start'),
  ('class_3_unlock_level','11','level at which the THIRD class unlocks (contributes spells)','PARTIALLY_VERIFIED',1,'user report 2026-07-17'),
  ('exaltation_extract_min_tier','4','item tier at which its effect can be extracted as an Exaltation augment (regen effects excluded so far)','VERIFIED_INGAME',1,'user report 2026-07-15, live game'),
  ('hp_model','ESTIMATOR','base-HP model: ESTIMATOR (community curves; default) | THOUGHT_EXPERIMENT (coefficient placeholder) | OFF','PARTIALLY_VERIFIED',1,'Mosscovered Legend''s EQL Stat Estimator v0.1.4 (import_stat_estimator.py); validated vs live screenshots'),
  ('mana_model','ESTIMATOR','base-mana model: ESTIMATOR (community curves; default) | OFF','PARTIALLY_VERIFIED',1,'Mosscovered Legend''s EQL Stat Estimator v0.1.4; mana within ~2-3% of live screenshots'),
  ('hp_per_level_by_class','WAR=40 BER=35 MNK=34 RNG=33 BST=33 ROG=32 BRD=32 PAL=30 SHD=30 CLR=30 SHM=30 DRU=29 ENC=24 MAG=24 NEC=24 WIZ=24','base HP per level per class (placeholder curve, top-2 blended)','NEEDS_INGAME_TEST',1,'calibrated 2026-07-23 vs IMG_8810/8811 anchors'),
  ('hp_sta_coeff_by_class','WAR=6 PAL=5 SHD=5 MNK=4 RNG=4 ROG=4 BRD=4 BST=4 BER=4 CLR=3.5 DRU=3.5 SHM=3.5 ENC=2 MAG=2 NEC=2 WIZ=2','HP per point of STA per class (archetype returns: tank 6, knights 5, melee/priest 3-4, casters 2)','NEEDS_INGAME_TEST',1,'user thought experiment 2026-07-23'),
  ('hp_weights','1.0,1.0,0.0','multiclass blend weights w1,w2,w3 (defaults = eqltools TOP2_SUM: top two full, third dropped)','NEEDS_INGAME_TEST',1,'eqltools TOP2_SUM + thought-experiment structure'),
  ('hp_post50_growth','1.10','exponential phase stub: class-base HP multiplier per level past 50','NEEDS_INGAME_TEST',1,'user thought experiment 2026-07-23 (phase 2 unmeasured)');

-- corrections for DBs seeded before 2026-07-16 (never clobber a user edit):
-- eqltools.com client-mined rules supersede the plan's guesses
UPDATE formula_table SET value_text='TOP2_SUM', verification_status='PARTIALLY_VERIFIED',
  description='HP from the TWO highest classes summed; the third is dropped',
  source='eqltools.com Attributes (client-mined; also applies to endurance)'
  WHERE formula_key='multi_class_hp_combine' AND dim1='' AND is_user_edited=0 AND value_text='BEST_OF';
UPDATE formula_table SET value_text='TOP2_SUM', verification_status='PARTIALLY_VERIFIED',
  description='mana from the TWO highest classes summed; a third caster adds NO mana',
  source='eqltools.com Attributes (client-mined)'
  WHERE formula_key='multi_class_mana_combine' AND dim1='' AND is_user_edited=0 AND value_text='BEST_OF';
UPDATE formula_table SET verification_status='PARTIALLY_VERIFIED',
  description='multiclass attribute combine: race base + each class''s +30 points, additive',
  source='eqltools.com Attributes (client-mined: each class contributes exactly 30)'
  WHERE formula_key='class_attr_combine' AND dim1='' AND is_user_edited=0 AND value_text='SUM'
    AND verification_status='NEEDS_INGAME_TEST';

-- the spell tier % guess of 10 is superseded by the community-reconstructed 6
UPDATE formula_table SET value_text='6', verification_status='PARTIALLY_VERIFIED',
  description='spell damage/healing: +6% of base per tier, LINEAR, floor (also scales buff values in this planner — stat-buff scaling itself is unverified)',
  source='community-reconstructed 2026-07 (Ice Comet 808 -> ~1050 @T5); official 7/7 notes confirm dmg+heal scale but not the %'
  WHERE formula_key='spell_tier_scaling_pct' AND dim1='' AND is_user_edited=0 AND value_text='10';

-- 2026-07-21: EQL Discord community-reported caps — buffed attributes 510 (was the old
-- 255 legacy guess), saves/resists 1000. Only touch the unedited legacy default.
UPDATE formula_table SET value_text='510', verification_status='PARTIALLY_VERIFIED',
  description='buffed attribute hard cap (STR..CHA)',
  source='EQL Discord community-reported 2026-07-21'
  WHERE formula_key='stat_cap' AND dim1='' AND is_user_edited=0 AND value_text='255';

-- 2026-07-23: the estimator curves supersede the thought-experiment placeholder as
-- the default base-HP model (only when the user hasn't edited the row)
UPDATE formula_table SET value_text='ESTIMATOR', verification_status='PARTIALLY_VERIFIED',
  description='base-HP model: ESTIMATOR (community curves; default) | THOUGHT_EXPERIMENT (coefficient placeholder) | OFF',
  source='Mosscovered Legend''s EQL Stat Estimator v0.1.4 (import_stat_estimator.py)'
  WHERE formula_key='hp_model' AND dim1='' AND is_user_edited=0 AND value_text='THOUGHT_EXPERIMENT';
