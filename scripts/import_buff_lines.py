#!/usr/bin/env python3
"""
import_buff_lines.py - Buff Lines page (pageid 50578) -> buff_line / buff_line_member
                       + spell_buff_line, as a verified=0 / NEEDS_INGAME_TEST seed.
Plan refs: character-builder-plan.md 2.1 (DDL), 3.6 (parser), risk 3.

OFFLINE by design: parses the cached wikitext (raw/fixtures/probe-spells/Buff_Lines.wikitext),
resolves member links against the existing `spell` table when present, and upserts the
buff-line tables into the target DB. Also applies overrides/seeds/pet_buff_lines.yaml
(pet lines have NO wiki source). Re-runnable: pre-deletes prior rows for this source.

Usage:
  python scripts/import_buff_lines.py --db db/eql.db [--wikitext PATH] [--revid N]
  python scripts/import_buff_lines.py --verify        # build a temp spell table from
                                                      # raw/spells.json and report coverage
"""
import os, re, sys, json, sqlite3, argparse

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WIKITEXT = os.path.join(BASE, "raw", "fixtures", "probe-spells", "Buff_Lines.wikitext")
PET_SEED = os.path.join(BASE, "overrides", "seeds", "pet_buff_lines.yaml")
PET_SEED_JSON = os.path.join(BASE, "overrides", "seeds", "pet_buff_lines.json")
BUFF_LINES_PAGEID = 50578

BUFF_GROUPS = {"Attribute Enhancing", "Other Buffs", "Resistances", "Speed"}
CLASSES = ["Cleric","Druid","Shaman","Enchanter","Magician","Necromancer","Wizard",
           "Bard","Ranger","Paladin","Warrior","Rogue","Monk","Beastlord","Berserker",
           "Shadow Knight"]
CLASS_RE = re.compile(r"\[\[(" + "|".join(CLASSES) + r")\]\]\s*(\d+)")
KIND_RE  = re.compile(r"(Proc|Click|Worn|Consumable)\s*::?", re.I)

# ------------------------------------------------------------------ DDL (plan 2.1)
DDL = """
CREATE TABLE IF NOT EXISTS buff_line (
  id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, category TEXT, statistic TEXT,
  effect_slot INTEGER, bard_layer INTEGER,
  selection_policy TEXT NOT NULL DEFAULT 'HIGHEST_EFFECT_VALUE'
    CHECK (selection_policy IN ('HIGHEST_EFFECT_VALUE','HIGHEST_PRIORITY','MANUAL_ONLY')),
  notes TEXT, verified INTEGER NOT NULL DEFAULT 0, source_revision INTEGER );
CREATE TABLE IF NOT EXISTS buff_line_member (
  id INTEGER PRIMARY KEY,
  buff_line_id INTEGER NOT NULL REFERENCES buff_line(id) ON DELETE CASCADE,
  spell_id INTEGER,
  member_name_raw TEXT,
  source_kind TEXT NOT NULL DEFAULT 'SPELL'
    CHECK (source_kind IN ('SPELL','CLICK','PROC','WORN','CONSUMABLE')),
  priority INTEGER, effect_value_reference TEXT, value_base REAL, value_max_instrument REAL,
  source_items TEXT, is_group INTEGER, is_self_only INTEGER, duration_note TEXT,
  gm_event INTEGER, combination_group_id INTEGER,
  verified INTEGER NOT NULL DEFAULT 0, source_revision INTEGER,
  UNIQUE (buff_line_id, spell_id, source_kind) );
CREATE TABLE IF NOT EXISTS spell_buff_line (
  spell_id INTEGER NOT NULL, buff_line_id INTEGER NOT NULL REFERENCES buff_line(id) ON DELETE CASCADE,
  relationship TEXT NOT NULL CHECK (relationship IN ('PRIMARY','CONSUMES_LINE','STACKS_WITH_LINE','EXCEPTION')),
  verified INTEGER NOT NULL DEFAULT 0, source_revision INTEGER,
  PRIMARY KEY (spell_id, buff_line_id) );
CREATE INDEX IF NOT EXISTS idx_blm_line  ON buff_line_member(buff_line_id);
CREATE INDEX IF NOT EXISTS idx_blm_spell ON buff_line_member(spell_id);
"""

# ------------------------------------------------------------------ helpers
def normalize(name):
    """Plan 3.7 shared normalizer: strip alias/paren/level suffix/'Spell:' prefix, casefold."""
    n = name.split("|")[0]                       # link target over display
    n = re.sub(r"^Spell:\s*", "", n, flags=re.I)
    n = re.sub(r"\s*\([^)]*\)\s*$", "", n)        # trailing parenthetical
    n = re.sub(r"\s+\d+$", "", n)                 # trailing level
    n = n.replace("’", "'").replace("`", "'").strip().rstrip(".")  # backtick = apostrophe
    return re.sub(r"\s+", " ", n).casefold()

def parse_line_name(h4, statistic):
    """Return (display_name, effect_slot, bard_layer). Handles 'AC (Slot 2)', 'Layer 2, Slot 3 & 4'."""
    slot = layer = None
    ms = re.search(r"Slot\s+(\d+)", h4);   slot  = int(ms.group(1)) if ms else None
    ml = re.search(r"Layer\s+(\d+)", h4);  layer = int(ml.group(1)) if ml else None
    # qualify with statistic unless the heading already leads with it, so names are UNIQUE
    # (handles bare resist headings 'Primary'/'Potion'/'Psalm'/'Item' and 'Slot 4'/'Layer ...')
    if statistic and not h4.startswith(statistic):
        name = f"{statistic} ({h4})"
    else:
        name = h4
    return name, slot, layer

def parse_bullet(text):
    """Parse one '* +val (+inst) [[Entity]] (annotations)' bullet."""
    m = re.match(r"\+?\s*(-?\d+)\s*(?:\(\+?(\d+)\))?\s*(.*)$", text.strip())
    if not m:
        return None
    value_base = float(m.group(1))
    value_max  = float(m.group(2)) if m.group(2) else None
    rest = m.group(3)
    lm = re.search(r"\[\[([^\]|]+)(?:\|[^\]]*)?\]\]", rest)
    if not lm:
        return None
    entity = lm.group(1).strip()
    annot  = rest[lm.end():]
    combo_mark = bool(re.search(r"'''\s*\*\s*'''", rest))
    # flags
    is_group   = 1 if re.search(r"\(\s*Group\s*\)", annot, re.I) else 0
    is_self    = 1 if re.search(r"Self[-\s]?only", annot, re.I) else 0
    gm_event   = 1 if re.search(r"GM Event", annot, re.I) else 0
    dm = re.search(r"[Ll]asts?[^,)]*", annot); duration = dm.group(0).strip() if dm else None
    has_class  = bool(CLASS_RE.search(annot))
    km = KIND_RE.search(annot)
    if has_class:
        source_kind = "SPELL"
    elif km:
        source_kind = km.group(1).upper()
    else:
        source_kind = "SPELL"
    # source_items = [[...]] in annotation that are NOT class links
    items = []
    for im in re.finditer(r"\[\[([^\]|]+)(?:\|[^\]]*)?\]\]", annot):
        tok = im.group(1).strip()
        if tok not in CLASSES:
            items.append(tok)
    return dict(value_base=value_base, value_max_instrument=value_max, entity=entity,
                effect_value_reference=text.strip()[:120], source_kind=source_kind,
                source_items=items, is_group=is_group, is_self_only=is_self,
                duration_note=duration, gm_event=gm_event, combo_mark=combo_mark)

# ------------------------------------------------------------------ parse page
def parse_page(wikitext):
    """Return list of lines: {name, statistic, category, effect_slot, bard_layer, members[]}."""
    group = statistic = None
    lines, cur = [], None
    for raw in wikitext.splitlines():
        h = re.match(r"^(={2,4})\s*(.*?)\s*=+\s*$", raw)
        if h:
            depth, title = len(h.group(1)), h.group(2).strip()
            if depth == 2:
                group = title; statistic = None; cur = None
            elif depth == 3:
                statistic = title; cur = None
                # a statistic with direct bullets becomes its own line (handled lazily below)
            elif depth == 4 and group in BUFF_GROUPS:
                name, slot, layer = parse_line_name(title, statistic or title)
                cur = dict(name=name, statistic=statistic, category=group,
                           effect_slot=slot, bard_layer=layer, members=[])
                lines.append(cur)
            continue
        if not raw.startswith("* "):
            continue
        if group not in BUFF_GROUPS:
            continue
        if cur is None:                            # bullets directly under an h3 statistic
            if statistic is None:
                continue
            name, slot, layer = parse_line_name(statistic, statistic)
            cur = dict(name=name, statistic=statistic, category=group,
                       effect_slot=slot, bard_layer=layer, members=[])
            lines.append(cur)
        mb = parse_bullet(raw[2:])
        if mb:
            cur["members"].append(mb)
    return [l for l in lines if l["members"]]

# ------------------------------------------------------------------ load
def load(con, lines, revid):
    con.executescript(DDL)
    # spell name -> id resolver (works whether or not a spell table exists).
    # Titles registered FIRST so an exact spellname entry wins collisions; wiki links
    # target page titles, so title matching fixes spellname-typo pages ('Manicial').
    resolver = {}
    try:
        for r in con.execute("SELECT id, page_title FROM spell WHERE page_title IS NOT NULL"):
            resolver[normalize(r[1])] = r[0]
        for r in con.execute("SELECT id, name FROM spell"):
            resolver[normalize(r[1])] = r[0]
    except sqlite3.OperationalError:
        pass
    # pre-delete prior page rows (idempotent) — everything not from the pet seed
    con.execute("DELETE FROM buff_line_member WHERE buff_line_id IN "
                "(SELECT id FROM buff_line WHERE category != 'PET_SEED')")
    con.execute("DELETE FROM buff_line WHERE category != 'PET_SEED'")

    resolved = unresolved = 0
    member_lines = {}   # normalized spell name -> set(buff_line_id) for combination detection
    for L in lines:
        cur = con.execute(
            "INSERT INTO buff_line(name,category,statistic,effect_slot,bard_layer,notes,verified,source_revision)"
            " VALUES(?,?,?,?,?,?,0,?) ON CONFLICT(name) DO UPDATE SET statistic=excluded.statistic,"
            " effect_slot=excluded.effect_slot,bard_layer=excluded.bard_layer,source_revision=excluded.source_revision"
            " RETURNING id", (L["name"], L["category"], L["statistic"], L["effect_slot"],
                              L["bard_layer"], None, revid))
        line_id = cur.fetchone()[0]
        seen = {}   # (spell_id/name, source_kind) dedup within a line -> keep strongest
        for prio, m in enumerate(L["members"], 1):
            sid = resolver.get(normalize(m["entity"]))
            if sid: resolved += 1
            else:   unresolved += 1
            key = (sid if sid else normalize(m["entity"]), m["source_kind"])
            if key in seen:      # UNIQUE(buff_line_id,spell_id,source_kind): keep first (strongest)
                continue
            seen[key] = 1
            con.execute(
                "INSERT OR IGNORE INTO buff_line_member(buff_line_id,spell_id,member_name_raw,"
                "source_kind,priority,effect_value_reference,value_base,value_max_instrument,"
                "source_items,is_group,is_self_only,duration_note,gm_event,verified,source_revision)"
                " VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,0,?)",
                (line_id, sid, None if sid else m["entity"], m["source_kind"], prio,
                 m["effect_value_reference"], m["value_base"], m["value_max_instrument"],
                 json.dumps(m["source_items"]) if m["source_items"] else None,
                 m["is_group"], m["is_self_only"], m["duration_note"], m["gm_event"], revid))
            member_lines.setdefault(normalize(m["entity"]), {}).setdefault(line_id, m["value_base"])
    # combination detection: a spell in >= 2 lines -> PRIMARY on largest, CONSUMES_LINE else
    con.execute("DELETE FROM spell_buff_line WHERE source_revision=? OR source_revision IS NULL", (revid,))
    gid = 0
    for nm, lid_vals in member_lines.items():
        if len(lid_vals) < 2:
            continue
        gid += 1
        sid = resolver.get(nm)
        primary_line = max(lid_vals, key=lambda k: lid_vals[k])
        for lid in lid_vals:
            con.execute("UPDATE buff_line_member SET combination_group_id=? WHERE buff_line_id=? "
                        "AND (spell_id=? OR member_name_raw=?)",
                        (gid, lid, sid, None if sid else nm))
            if sid:
                rel = "PRIMARY" if lid == primary_line else "CONSUMES_LINE"
                con.execute("INSERT OR IGNORE INTO spell_buff_line(spell_id,buff_line_id,relationship,verified,source_revision)"
                            " VALUES(?,?,?,0,?)", (sid, lid, rel, revid))
    con.commit()
    return resolved, unresolved, gid

# ------------------------------------------------------------------ pet seed
def load_pet_seed(con):
    data = None
    if os.path.exists(PET_SEED):
        try:
            import yaml
            data = yaml.safe_load(open(PET_SEED, encoding="utf-8"))
        except Exception:
            data = None
    if data is None and os.path.exists(PET_SEED_JSON):
        data = json.load(open(PET_SEED_JSON, encoding="utf-8"))
    if not data:
        print("  (pet seed not found; skipping)"); return 0, 0
    resolver = {}
    try:
        for r in con.execute("SELECT id, page_title FROM spell WHERE page_title IS NOT NULL"):
            resolver[normalize(r[1])] = r[0]
        for r in con.execute("SELECT id, name FROM spell"):
            resolver[normalize(r[1])] = r[0]
    except sqlite3.OperationalError:
        pass
    con.execute("DELETE FROM buff_line_member WHERE buff_line_id IN (SELECT id FROM buff_line WHERE category='PET_SEED')")
    con.execute("DELETE FROM buff_line WHERE category='PET_SEED'")
    nl = nm = 0
    for L in data.get("lines", []):
        lid = con.execute("INSERT INTO buff_line(name,category,statistic,selection_policy,notes,verified,source_revision)"
                          " VALUES(?,?,?,?,?,0,NULL) ON CONFLICT(name) DO UPDATE SET notes=excluded.notes RETURNING id",
                          (L["name"], "PET_SEED", L.get("statistic"),
                           L.get("selection_policy","HIGHEST_EFFECT_VALUE"),
                           L.get("notes","pet buff line — no wiki source; NEEDS_INGAME_TEST"))).fetchone()[0]
        nl += 1
        for prio, mem in enumerate(L.get("members", []), 1):
            sid = resolver.get(normalize(mem["spell"]))
            con.execute("INSERT OR IGNORE INTO buff_line_member(buff_line_id,spell_id,member_name_raw,"
                        "source_kind,priority,value_base,verified,source_revision) VALUES(?,?,?,?,?,?,0,NULL)",
                        (lid, sid, None if sid else mem["spell"], mem.get("source_kind","SPELL"),
                         mem.get("priority",prio), mem.get("value_base")))
            nm += 1
    con.commit()
    return nl, nm

# ------------------------------------------------------------------ main
def build_temp_spell_table(con):
    """--verify helper: create a spell table from raw/spells.json titles for resolution testing."""
    con.execute("CREATE TABLE IF NOT EXISTS spell(id INTEGER PRIMARY KEY, name TEXT)")
    data = json.load(open(os.path.join(BASE,"raw","spells.json"), encoding="utf-8"))
    for row in data:
        con.execute("INSERT OR IGNORE INTO spell(id,name) VALUES(?,?)",
                    (int(row["pageid"]), row["title"]))
    con.commit()

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=os.environ.get("EQL_DB", os.path.join(BASE,"db","eql.db")))
    ap.add_argument("--wikitext", default=WIKITEXT)
    ap.add_argument("--revid", type=int, default=None)
    ap.add_argument("--verify", action="store_true",
                    help="use a temp DB with a spell table built from raw/spells.json")
    a = ap.parse_args()
    dbpath = "/tmp/eql_bufftest.db" if a.verify else a.db
    if a.verify and os.path.exists(dbpath): os.remove(dbpath)
    con = sqlite3.connect(dbpath)
    if a.verify:
        build_temp_spell_table(con)
    wt = open(a.wikitext, encoding="utf-8").read()
    lines = parse_page(wt)
    res, unres, ncombo = load(con, lines, a.revid)
    npl, npm = load_pet_seed(con)
    tot = res + unres
    print(f"Buff Lines import ({'VERIFY temp db' if a.verify else dbpath}):")
    print(f"  buff_line rows       : {len(lines)}")
    print(f"  member rows          : {tot}  (resolved {res}, unresolved {unres}, "
          f"{100*res/tot:.1f}% linked to spells)")
    print(f"  combination groups   : {ncombo}")
    print(f"  pet seed lines/members: {npl}/{npm}")
    # small provenance sample
    print("  sample lines:")
    for r in con.execute("SELECT name,statistic,effect_slot,bard_layer,"
                         "(SELECT count(*) FROM buff_line_member m WHERE m.buff_line_id=buff_line.id) n"
                         " FROM buff_line WHERE category!='PET_SEED' ORDER BY id LIMIT 6"):
        print(f"     - {r[0]:34} stat={r[1]} slot={r[2]} layer={r[3]} members={r[4]}")

if __name__ == "__main__":
    main()
