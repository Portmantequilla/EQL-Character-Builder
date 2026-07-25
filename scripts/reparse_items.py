#!/usr/bin/env python3
"""reparse_items.py — OFFLINE derivation pass over items already in db/eql.db.

Reads items.raw_statsblock + items.click_effect/worn_effect/focus_effect and
derives (idempotent, re-runnable, NO network):

  items.required_level / items.recommended_level   ("Required level of N." lines)
  items.instrument_type / items.instrument_resonance
  item_deity(pageid, deity)                        ("Deity:"/"Diety:" lines)
  item_effect(pageid, effect_name, activation_type, required_level, spell_id)
  item_slots(pageid, slot)                         (canonical slot vocabulary)

Also normalizes backtick->apostrophe in drops.item_name/mob_name, but ONLY where
the apostrophe spelling exists in items/mobs and the backtick spelling does not
(94 item names and ~200 mob names legitimately contain backticks on the wiki —
a blanket REPLACE would break those joins).

NOTE (data reality): the wiki only carries an item-level "Required level of N"
line on ~16 items (6 epics say "Required level of 46."; most epic pages carry
NO required-level line at all — verified against the raw wikitext). The
"Required Level: N" colon-style lines are effect continuation lines (cast time /
cooldown blocks) and are attributed to the item's effect, not the item.

Usage:  python scripts/reparse_items.py [--db db/eql.db]
"""
import os, re, sys, argparse, sqlite3

SCRIPTS = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, SCRIPTS)
from eql_wiki_sync import canonical_name  # noqa: E402  (casefold+ws+backtick fold)

BASE = os.path.dirname(SCRIPTS)
DB = os.environ.get("EQL_DB", os.path.join(BASE, "db", "eql.db"))

# --------------------------------------------------------------------- helpers
BR_SPLIT = re.compile(r"<br\s*/?\s*>", re.I)
TAG = re.compile(r"<[^>]+>")

def lines_of(raw):
    out = []
    for chunk in BR_SPLIT.split(raw or ""):
        for ln in chunk.splitlines():
            ln = TAG.sub("", ln).replace("&nbsp;", " ").strip()
            if ln:
                out.append(ln)
    return out

def column_exists(cur, table, col):
    return any(r[1] == col for r in cur.execute("PRAGMA table_info(%s)" % table))

def ensure_column(cur, table, col, decl):
    if not column_exists(cur, table, col):
        cur.execute("ALTER TABLE %s ADD COLUMN %s %s" % (table, col, decl))
        print("  added column %s.%s" % (table, col))

# ----------------------------------------------------------- a) required level
# Item-level lines only. "Required Level: N" (colon form) belongs to effect
# cast-time/cooldown blocks and is handled by the effect parser below.
REQ_LEVEL = re.compile(r"\bRequired level of (\d+)", re.I)
REC_LEVEL = re.compile(r"\bRecommended level of (\d+)", re.I)

# ------------------------------------------------------------------- b) deity
KNOWN_DEITIES = [  # longest-first matching below
    "Bertoxxulous", "Brell Serilis", "Bristlebane", "Cazic-Thule",
    "Erollisi Marr", "Innoruuk", "Karana", "Mithaniel Marr", "Prexus",
    "Quellious", "Rallos Zek", "Rodcet Nife", "Solusek Ro", "The Tribunal",
    "Tunare", "Veeshan", "Agnostic",
]
DEITY_ALIASES = {"Cazic Thule": "Cazic-Thule", "Fizzlethorpe Bristlebane": "Bristlebane"}
# 'D(ei|ie)ty' — the wiki misspells 'Diety:'/'Dieties:' on a handful of pages
DEITY_LINE = re.compile(r"^D(?:ei|ie)t(?:y|ies)\s*:\s*(.+)$", re.I)
LINK = re.compile(r"\[\[([^\]|]+)(?:\|[^\]]*)?\]\]")

def parse_deities(text):
    """Extract known deity names from the free text after 'Deity:'.
    Handles [[links]], '/', ',', and space-separated multi-deity lists."""
    s = LINK.sub(lambda m: m.group(1), text)
    for k, v in DEITY_ALIASES.items():
        s = re.sub(re.escape(k), v, s, flags=re.I)
    found, remainder = [], s
    for d in sorted(KNOWN_DEITIES, key=len, reverse=True):
        pat = re.compile(re.escape(d), re.I)
        if pat.search(remainder):
            found.append(d)
            remainder = pat.sub(" ", remainder)
    leftover = re.sub(r"[\s,/.]+", " ", remainder).strip()
    return found, leftover

# -------------------------------------------------------------- c) instruments
# 'Brass Resonance: 14' | 'Wind Instrument' | 'Brass Instruments: 10'
# 'Stringed Instrument: 10' | 'String Resonance: 15' | 'All Instrument Types'
INSTRUMENT = re.compile(
    r"^(Brass|Wind|String(?:ed)?|Percussion|Singing)\s+"
    r"(?:Instruments?|Resonance)s?\s*:?\s*(\d+)?\s*$", re.I)
ALL_INSTRUMENT = re.compile(r"^All Instrument(?:s| Types?)?\s*:?\s*(\d+)?\s*$", re.I)
INSTR_CANON = {"BRASS": "BRASS", "WIND": "WIND", "STRING": "STRINGED",
               "STRINGED": "STRINGED", "PERCUSSION": "PERCUSSION", "SINGING": "SINGING"}

def parse_instrument(raw_lines):
    for ln in raw_lines:
        m = INSTRUMENT.match(ln)
        if m:
            fam = INSTR_CANON[m.group(1).upper()]
            return fam, (int(m.group(2)) if m.group(2) else None)
        m = ALL_INSTRUMENT.match(ln)
        if m:
            return "ALL", (int(m.group(1)) if m.group(1) else None)
    return None, None

# ------------------------------------------------------------------ d) effects
# Level suffix variants observed in the data:
#   'at Level 50' | 'as Level 45' | ') at 45' | 'req. level 20' | '(Level 1)'
#   '(..., Level 40)' | 'Required Level: 46' (inline) | '(Req Level 30)'
EFF_LEVEL_PATTERNS = [
    re.compile(r"[\s,]*(?:at|as)\s+Level\s+(\d+)\s*\.?\s*$", re.I),
    re.compile(r"[\s,]*req\.?\s*level\s+(\d+)\s*\.?\s*$", re.I),
    re.compile(r"\)\s*at\s+(\d+)\s*\.?\s*$", re.I),          # ') at 45'
    re.compile(r"\s*\(\s*Level\s+(\d+)\s*\)\s*$", re.I),      # '(Level 1)'
    re.compile(r"\(\s*Req\.?\s*Level:?\s*(\d+)\s*\)", re.I),  # '(Req Level 30)'
    re.compile(r"Required Level:\s*(\d+)", re.I),             # inline continuation
]
# standalone continuation lines under a 'Click Effect:' block (57121-style):
#   Cast Time: Instant / Required Level: 46 / Cooldown: 120 seconds
CONT_REQ_LEVEL = re.compile(r"Required Level:\s*(\d+)", re.I)
EFFECT_LINE = re.compile(r"(?:(?:click|worn|focus|combat|proc)\s+)?effect\s*:", re.I)
PROC_PREFIX_LINE = re.compile(r"(?:combat|proc)\s+effect\s*:", re.I)
# ', Level 40' hiding INSIDE the parenthetical (stripped with the paren anyway)
PAREN_LEVEL = re.compile(r"[(,]\s*Level\s+(\d+)\s*[),]", re.I)
# parentheticals that are activation/casting metadata, not part of the name
META_PAREN = re.compile(
    r"\s*\((?=[^)]*(?:Any Slot|Must Equip|Can Equip|Combat|Worn|Casting Time|"
    r"Cast Time|Cooldown|Instant|Level|Triggered|Proc))[^)]*\)", re.I)
LINK_FULL = re.compile(r"\[\[([^\]|]+)(?:\|[^\]]*)?\]\]")
RESONANCE_NOISE = re.compile(
    r"^(Brass|Wind|String(?:ed)?|Percussion|Singing|All)\s+(Instrument|Resonance)", re.I)

def parse_effect(text, default_activation):
    """-> (effect_name, activation_type, required_level) or None to skip."""
    s = (text or "").strip()
    if not s:
        return None
    plain = TAG.sub("", LINK_FULL.sub(lambda m: m.group(1), s)).strip()
    if RESONANCE_NOISE.match(plain):
        return None  # a resonance line mis-filed as an effect; modeled elsewhere
    level = None
    for pat in EFF_LEVEL_PATTERNS:
        m = pat.search(s)
        if m:
            level = int(m.group(1))
            s = pat.sub("", s).strip()
            break
    if level is None:
        m = PAREN_LEVEL.search(s)
        if m:
            level = int(m.group(1))
    activation = default_activation
    if default_activation in ("CLICK",) and re.search(r"\(\s*Combat\b", s, re.I):
        activation = "PROC"
    if re.search(r"\(\s*Worn\b", s, re.I):
        activation = "WORN"
    # name: prefer the wiki link target (canonical spell name)
    m = LINK_FULL.search(s)
    if m:
        name = m.group(1).strip()
    else:
        name = META_PAREN.sub("", TAG.sub("", s))
        name = re.sub(r"\s*-\s*$", "", name).strip(" .,-")
    # wiki-title artifacts: underscores + '(Spell)'/'(Effect)' disambiguators
    # (spell.name is the clean form; spell.page_title keeps the disambiguator)
    name = name.replace("_", " ")
    name = re.sub(r"\s*\((?:spell|effect|item)\)\s*$", "", name, flags=re.I)
    name = re.sub(r"\s+", " ", name).strip()
    if not name:
        return None
    return name, activation, level

# -------------------------------------------------------------------- e) slots
SLOT_VOCAB = ["EAR", "HEAD", "FACE", "NECK", "SHOULDERS", "ARMS", "BACK",
              "WRIST", "RANGE", "HANDS", "PRIMARY", "SECONDARY", "FINGER",
              "CHEST", "LEGS", "FEET", "WAIST", "AMMO", "CHARM"]
SLOT_TOKEN_MAP = {v: v for v in SLOT_VOCAB}
SLOT_TOKEN_MAP.update({
    "FINGERS": "FINGER", "EARS": "EAR", "WRISTS": "WRIST",
    "SECONDAY": "SECONDARY", "SECONDARY.": "SECONDARY",   # wiki typo
    "SHOULDER": "SHOULDERS", "ARM": "ARMS", "LEG": "LEGS", "HAND": "HANDS",
    "RANGED": "RANGE",
})

def parse_slots(slot_text):
    """-> (set(canonical), list(unmapped tokens))"""
    s = TAG.sub(" ", slot_text or "")
    s = s.replace(",", " ").replace("/", " ")
    toks = [t for t in re.split(r"\s+", s.upper()) if t]
    mapped, unmapped = set(), []
    for t in toks:
        t = t.strip(".;:")
        if not t:
            continue
        if t in SLOT_TOKEN_MAP:
            mapped.add(SLOT_TOKEN_MAP[t])
        else:
            unmapped.append(t)
    return mapped, unmapped

# ----------------------------------------------------------------------- main
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=DB)
    args = ap.parse_args()

    con = sqlite3.connect(args.db)
    cur = con.cursor()

    print("== schema ==")
    ensure_column(cur, "items", "required_level", "INTEGER")
    ensure_column(cur, "items", "recommended_level", "INTEGER")
    ensure_column(cur, "items", "instrument_type", "TEXT")
    ensure_column(cur, "items", "instrument_resonance", "INTEGER")
    cur.execute("""CREATE TABLE IF NOT EXISTS item_deity(
        pageid INTEGER, deity TEXT, PRIMARY KEY(pageid, deity))""")
    cur.execute("""CREATE TABLE IF NOT EXISTS item_effect(
        id INTEGER PRIMARY KEY, pageid INTEGER, effect_name TEXT,
        activation_type TEXT CHECK(activation_type IN ('CLICK','WORN','FOCUS','PROC')),
        required_level INTEGER, spell_id INTEGER)""")
    ensure_column(cur, "item_effect", "spell_id", "INTEGER")  # pre-existing DBs
    cur.execute("""CREATE TABLE IF NOT EXISTS item_slots(
        pageid INTEGER, slot TEXT, PRIMARY KEY(pageid, slot))""")

    # idempotent rebuild: clear derived rows/values, never DROP
    cur.execute("DELETE FROM item_deity")
    cur.execute("DELETE FROM item_effect")
    cur.execute("DELETE FROM item_slots")
    cur.execute("""UPDATE items SET required_level=NULL, recommended_level=NULL,
                   instrument_type=NULL, instrument_resonance=NULL""")

    deity_rows, effect_rows, slot_rows = [], [], []
    n_req = n_rec = n_instr = 0
    deity_leftovers, unmapped_slots = {}, {}
    slotted = 0

    for pid, slot, raw, click, worn, focus in cur.execute(
            """SELECT pageid, slot, raw_statsblock, click_effect, worn_effect,
                      focus_effect FROM items""").fetchall():
        raw_lines = lines_of(raw)

        # a) item-level required / recommended level
        req = rec = None
        for ln in raw_lines:
            m = REQ_LEVEL.search(ln)
            if m and req is None:
                req = int(m.group(1))
            m = REC_LEVEL.search(ln)
            if m and rec is None:
                rec = int(m.group(1))
        # c) instrument — statsblock lines first; a few bard items carry the
        # resonance only in an effect string ('Percussion Resonance 14')
        itype, ires = parse_instrument(raw_lines)
        if itype is None:
            eff_lines = [TAG.sub("", LINK.sub(lambda m: m.group(1), t)).strip()
                         for t in (click, worn, focus) if t]
            itype, ires = parse_instrument(eff_lines)
        if req is not None or rec is not None or itype is not None:
            cur.execute("""UPDATE items SET required_level=?, recommended_level=?,
                           instrument_type=?, instrument_resonance=? WHERE pageid=?""",
                        (req, rec, itype, ires, pid))
            n_req += req is not None
            n_rec += rec is not None
            n_instr += itype is not None

        # b) deities
        for ln in raw_lines:
            m = DEITY_LINE.match(ln)
            if m:
                found, leftover = parse_deities(m.group(1))
                for d in found:
                    deity_rows.append((pid, d))
                if leftover:
                    deity_leftovers.setdefault(leftover, []).append(pid)

        # d) effects — 'Combat Effect:'/'Proc Effect:' statsblock lines mean the
        # click column actually holds a weapon proc
        click_act = "CLICK"
        if any(PROC_PREFIX_LINE.match(ln) for ln in raw_lines):
            click_act = "PROC"
        parsed_effects = []
        for text, default_act in ((click, click_act), (worn, "WORN"), (focus, "FOCUS")):
            if text is None:
                continue
            parsed = parse_effect(text, default_act)
            if parsed:
                parsed_effects.append(list(parsed))
        # continuation-style 'Required Level: N' on its own line (57121-style
        # Cast Time / Required Level / Cooldown blocks): attach only when the
        # item has exactly one effect and the lines agree on one level
        if len(parsed_effects) == 1 and parsed_effects[0][2] is None:
            lvls = {int(m.group(1)) for ln in raw_lines
                    if not EFFECT_LINE.match(ln)
                    for m in [CONT_REQ_LEVEL.search(ln)] if m}
            if len(lvls) == 1:
                parsed_effects[0][2] = lvls.pop()
        for name, act, lvl in parsed_effects:
            effect_rows.append((pid, name, act, lvl))

        # e) slots
        if slot and slot.strip():
            slotted += 1
            mapped, unmapped = parse_slots(slot)
            for sl in mapped:
                slot_rows.append((pid, sl))
            if unmapped:
                unmapped_slots.setdefault(slot.strip(), []).append(pid)

    # f) resolve item_effect.spell_id against the spell domain:
    # exact canonical match on spell.name / spell.page_title, then retry with a
    # ' (Spell)' suffix (Kilva's Skin of Flame (Spell)), then the colon-insertion
    # variant 'Illusion X' -> 'Illusion: X' / 'Vocarate X' -> 'Vocarate: X'
    by_name, by_title = {}, {}
    for sid, sname, stitle in cur.execute(
            "SELECT id, name, page_title FROM spell").fetchall():
        by_name.setdefault(canonical_name(sname), sid)
        if stitle:
            by_title.setdefault(canonical_name(stitle), sid)

    def resolve_spell(effect_name):
        c = canonical_name(effect_name)
        candidates = [c, c + " (spell)"]
        m = re.match(r"^(illusion|vocarate)\s+(.+)$", c)
        if m:
            candidates.append("%s: %s" % (m.group(1), m.group(2)))
        for key in candidates:
            if key in by_name:
                return by_name[key]
            if key in by_title:
                return by_title[key]
        return None

    cur.executemany("INSERT OR IGNORE INTO item_deity(pageid, deity) VALUES(?,?)",
                    deity_rows)
    cur.executemany("""INSERT INTO item_effect(pageid, effect_name, activation_type,
                       required_level, spell_id) VALUES(?,?,?,?,?)""",
                    [(pid, name, act, lvl, resolve_spell(name))
                     for pid, name, act, lvl in effect_rows])
    cur.executemany("INSERT OR IGNORE INTO item_slots(pageid, slot) VALUES(?,?)",
                    slot_rows)

    # g) drops backtick normalization — ONLY where the apostrophe spelling exists
    # in items/mobs and the backtick spelling does not (94 item / ~200 mob names
    # legitimately keep backticks; a blanket REPLACE would break those joins)
    def fix_drops_column(col, ref_table):
        fixed = 0
        rows = cur.execute(
            """SELECT mob_name, item_name FROM drops WHERE %s LIKE '%%`%%'
               AND EXISTS(SELECT 1 FROM %s r
                          WHERE r.name = REPLACE(drops.%s, '`', ''''))
               AND NOT EXISTS(SELECT 1 FROM %s r WHERE r.name = drops.%s)"""
            % (col, ref_table, col, ref_table, col)).fetchall()
        for mob, item in rows:
            new_mob = mob.replace("`", "'") if col == "mob_name" else mob
            new_item = item.replace("`", "'") if col == "item_name" else item
            rar = cur.execute("""SELECT rarity FROM drops WHERE mob_name=?
                                 AND item_name=?""", (mob, item)).fetchone()[0]
            cur.execute("DELETE FROM drops WHERE mob_name=? AND item_name=?",
                        (mob, item))
            cur.execute("""INSERT OR REPLACE INTO drops(mob_name, item_name, rarity)
                           VALUES(?,?,?)""", (new_mob, new_item, rar))
            fixed += 1
        return fixed

    fixed_items = fix_drops_column("item_name", "items")
    fixed_mobs = fix_drops_column("mob_name", "mobs")
    con.commit()

    # ------------------------------------------------------------------ report
    def table_count(t):
        return cur.execute("SELECT COUNT(*) FROM %s" % t).fetchone()[0]

    print("\n== report ==")
    print("items.required_level set on %d items (46 on %d)" %
          (n_req, cur.execute(
              "SELECT COUNT(*) FROM items WHERE required_level=46").fetchone()[0]))
    print("items.recommended_level set on %d items" % n_rec)
    print("items.instrument_type set on %d items" % n_instr)
    print("item_deity rows: %d (distinct items: %d)" %
          (table_count("item_deity"), cur.execute(
              "SELECT COUNT(DISTINCT pageid) FROM item_deity").fetchone()[0]))
    print("item_effect rows: %d (with required_level: %d; spell_id resolved: %d)" %
          (table_count("item_effect"), cur.execute(
              "SELECT COUNT(*) FROM item_effect WHERE required_level IS NOT NULL"
          ).fetchone()[0], cur.execute(
              "SELECT COUNT(*) FROM item_effect WHERE spell_id IS NOT NULL"
          ).fetchone()[0]))
    print("item_slots rows: %d" % table_count("item_slots"))
    print("drops normalized backtick->apostrophe: %d item_name, %d mob_name rows"
          % (fixed_items, fixed_mobs))

    print("\n-- samples: items with required/recommended level --")
    for r in cur.execute("""SELECT pageid, name, required_level, recommended_level
                            FROM items WHERE required_level IS NOT NULL
                            OR recommended_level IS NOT NULL LIMIT 5"""):
        print("  ", r)
    print("-- samples: item_deity --")
    for r in cur.execute("SELECT pageid, deity FROM item_deity LIMIT 5"):
        print("  ", r)
    print("-- samples: instruments --")
    for r in cur.execute("""SELECT pageid, name, instrument_type, instrument_resonance
                            FROM items WHERE instrument_type IS NOT NULL LIMIT 5"""):
        print("  ", r)
    print("-- samples: item_effect --")
    for r in cur.execute("""SELECT pageid, effect_name, activation_type, required_level
                            FROM item_effect LIMIT 5"""):
        print("  ", r)
    print("-- samples: item_slots --")
    for r in cur.execute("SELECT pageid, slot FROM item_slots LIMIT 5"):
        print("  ", r)

    print("\n-- item_effect by activation_type --")
    for r in cur.execute("""SELECT activation_type, COUNT(*) FROM item_effect
                            GROUP BY activation_type"""):
        print("  ", r)
    print("-- instrument_type distribution --")
    for r in cur.execute("""SELECT instrument_type, COUNT(*) FROM items
                            WHERE instrument_type IS NOT NULL GROUP BY 1"""):
        print("  ", r)
    print("-- item_deity distribution --")
    for r in cur.execute("""SELECT deity, COUNT(*) FROM item_deity
                            GROUP BY deity ORDER BY 2 DESC"""):
        print("  ", r)

    if deity_leftovers:
        print("\n-- UNPARSED deity text fragments --")
        for k, pids in sorted(deity_leftovers.items()):
            print("  %r on pageids %s" % (k, pids[:10]))
    else:
        print("\nall deity lines parsed cleanly")

    unresolved = cur.execute(
        """SELECT effect_name, COUNT(*) FROM item_effect WHERE spell_id IS NULL
           GROUP BY effect_name ORDER BY 2 DESC, 1""").fetchall()
    print("\n-- item_effect spell_id: %d unresolved rows (%d distinct names) --"
          % (sum(c for _, c in unresolved), len(unresolved)))
    for name, c in unresolved:
        print("  %dx %r" % (c, name))

    remaining_bt = cur.execute(
        """SELECT COUNT(*) FROM drops WHERE
           (item_name LIKE '%`%'
            AND EXISTS(SELECT 1 FROM items i
                       WHERE i.name = REPLACE(drops.item_name,'`',''''))
            AND NOT EXISTS(SELECT 1 FROM items i WHERE i.name = drops.item_name))
           OR (mob_name LIKE '%`%'
            AND EXISTS(SELECT 1 FROM mobs m
                       WHERE m.name = REPLACE(drops.mob_name,'`',''''))
            AND NOT EXISTS(SELECT 1 FROM mobs m WHERE m.name = drops.mob_name))"""
    ).fetchone()[0]
    print("\ndrops rows still backtick-mismatched (must be 0): %d" % remaining_bt)

    n_unmapped_items = sum(len(v) for v in unmapped_slots.values())
    covered = cur.execute("""SELECT COUNT(*) FROM items i WHERE slot IS NOT NULL
                             AND TRIM(slot)<>'' AND EXISTS
                             (SELECT 1 FROM item_slots s WHERE s.pageid=i.pageid)"""
                          ).fetchone()[0]
    print("\n-- slot coverage --")
    print("items with slot text: %d; with >=1 canonical slot row: %d (%.2f%%)" %
          (slotted, covered, 100.0 * covered / max(slotted, 1)))
    if unmapped_slots:
        print("distinct slot strings with unmapped tokens (%d items, %.2f%% of slotted):"
              % (n_unmapped_items, 100.0 * n_unmapped_items / max(slotted, 1)))
        for k, pids in sorted(unmapped_slots.items()):
            print("  %r  items=%d  e.g. pageids %s" % (k, len(pids), pids[:5]))
    else:
        print("no unmapped slot strings")

    con.close()
    print("\ndone.")

if __name__ == "__main__":
    main()
