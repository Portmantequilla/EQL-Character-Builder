-- EQ Legends wiki mirror - SQLite schema (built by eql_wiki_sync.py)
-- Items are keyed by MediaWiki pageid. Stats/classes/races/categories are
-- normalized child tables. Era is stored per row so out-of-era items carry over.

CREATE TABLE items(
  pageid INTEGER PRIMARY KEY, name TEXT UNIQUE, icon_id INTEGER,
  slot TEXT, weapon_skill TEXT, atk_delay INTEGER, dmg INTEGER, ac INTEGER,
  haste_pct INTEGER, worn_effect TEXT, focus_effect TEXT, click_effect TEXT,
  flags TEXT, era TEXT, notes TEXT, merchant_value TEXT, raw_statsblock TEXT,
  updated TEXT);
CREATE TABLE item_stats(pageid INTEGER, stat TEXT, value INTEGER, PRIMARY KEY(pageid,stat));
CREATE TABLE item_classes(pageid INTEGER, class TEXT, PRIMARY KEY(pageid,class));
CREATE TABLE item_races(pageid INTEGER, race TEXT, PRIMARY KEY(pageid,race));
CREATE TABLE item_categories(pageid INTEGER, category TEXT, PRIMARY KEY(pageid,category));
CREATE TABLE mobs(
  pageid INTEGER PRIMARY KEY, name TEXT UNIQUE, race TEXT, class TEXT, level TEXT,
  zone TEXT, loc TEXT, respawn TEXT, hp TEXT, dmg_per_hit TEXT,
  attacks_per_round TEXT, attack_speed TEXT, special TEXT, era TEXT, updated TEXT);
CREATE TABLE drops(mob_name TEXT, item_name TEXT, rarity TEXT, PRIMARY KEY(mob_name,item_name));
CREATE TABLE sync_meta(key TEXT PRIMARY KEY, value TEXT);

-- one row per (item, class) it can be worn by
CREATE VIEW v_item_class AS
  SELECT i.pageid,i.name,i.slot,i.era,c.class,i.worn_effect,i.focus_effect,i.haste_pct,i.ac,i.dmg
  FROM items i JOIN item_classes c ON c.pageid=i.pageid;
