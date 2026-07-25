#!/usr/bin/env python3
"""
resolve_buffs.py - REFERENCE stacking resolver (plan engine step 9: "strongest available
member per buff line"). Pure, deterministic, verifiable in Python. Serves two jobs:

  1. Interim USABLE tool: prints/exports a concrete buff plan for a triple-class build.
  2. GOLDEN ORACLE for the Rust M2 engine port (plan risk 8: "same pytest corpus").

This is NOT the production engine (that stays pure Rust per plan §4.1) — it is the
executable spec the Rust port is tested against. Inputs are the verified data files:
  exports/buff_lines.json  (from import_buff_lines.py)
  raw/spells.json          (for class-castability: '* [[Shaman]] - Level 57')

Usage:
  python scripts/resolve_buffs.py SHD MNK SHM [--level 60] [--target PLAYER|PET]
                                              [--bard-in-group] [--json out.json]
"""
import os, re, sys, json, argparse

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CLASS_ABBR = {
    "Warrior":"WAR","Cleric":"CLR","Paladin":"PAL","Ranger":"RNG","Shadow Knight":"SHD",
    "Druid":"DRU","Monk":"MNK","Bard":"BRD","Rogue":"ROG","Shaman":"SHM","Necromancer":"NEC",
    "Wizard":"WIZ","Magician":"MAG","Enchanter":"ENC","Beastlord":"BST","Berserker":"BER",
}
CLEVEL_RE = re.compile(
    r"\[\[(" + "|".join(map(re.escape, CLASS_ABBR)) + r")\]\]\s*(?:-\s*Level\s*|\(?Level\s*|\s)(\d+)")

# ---------------------------------------------------------------- data loaders
def load_spell_names(path):
    return {int(r["pageid"]): r["title"] for r in json.load(open(path, encoding="utf-8"))}

def load_spell_class_levels(path):
    """pageid -> {class_abbr: min_level} parsed from each spell page's `classes` field."""
    out = {}
    for row in json.load(open(path, encoding="utf-8")):
        wt = row["wikitext"]
        m = re.search(r"\|\s*classes\s*=(.*?)(?:\n\|\s*\w|\}\}\s*$)", wt, re.S)
        seg = m.group(1) if m else wt
        cl = {}
        for cm in CLEVEL_RE.finditer(seg):
            ab = CLASS_ABBR[cm.group(1)]; lv = int(cm.group(2))
            cl[ab] = min(lv, cl.get(ab, 999))
        if cl:
            out[int(row["pageid"])] = cl
    return out

# ---------------------------------------------------------------- resolver
def availability(member, build, level, clevels, bard_in_group):
    """Classify one member for a build. Returns (status, why, effective_value)."""
    val = member.get("value_base")
    inst = member.get("value_max_instrument")
    # bard instrument-scaled value only counts if a bard is in the group to play it
    eff = inst if (bard_in_group and inst is not None) else val
    sid = member.get("spell_id")
    kind = member["source_kind"]
    has_items = bool(member.get("source_items"))

    if kind in ("CLICK", "PROC", "WORN", "CONSUMABLE"):
        return "ITEM", f"{kind.lower()} item", eff
    # SPELL member: self-castable if a build class learns it by `level`
    if sid is not None and sid in clevels:
        castable = {c: lv for c, lv in clevels[sid].items() if c in build and lv <= level}
        if castable:
            c = min(castable, key=castable.get)
            return "SELF_CAST", f"cast by {c} (L{castable[c]})", eff
    if has_items:
        return "ITEM", "item-granted (click/proc/worn)", eff
    # castable by some class, just not one of ours -> needs an outside caster
    if sid is not None and sid in clevels:
        others = ",".join(sorted(clevels[sid]))
        return "EXTERNAL", f"only castable by {others}", eff
    return "UNKNOWN", "no class-level data / unresolved", eff

def resolve(build, level, target, bard_in_group, lines, clevels, names=None):
    """Return per-line resolution: chosen strongest available member + alternatives."""
    build = [c.upper() for c in build]
    results = []
    for L in lines:
        is_pet = L["category"] == "PET_SEED"
        if (target == "PET") != is_pet:
            continue
        avail = []
        for m in L["members"]:
            status, why, eff = availability(m, build, level, clevels, bard_in_group)
            sid = m.get("spell_id")
            name = (names or {}).get(sid) or m.get("member_name_raw") or f"spell#{sid}"
            avail.append(dict(name=name, status=status, why=why, value=eff,
                              self_only=m.get("is_self_only"), group=m.get("is_group"),
                              source_kind=m["source_kind"]))
        usable = [a for a in avail if a["status"] in ("SELF_CAST", "ITEM")
                  and a["value"] is not None]
        usable.sort(key=lambda a: a["value"], reverse=True)
        chosen = usable[0] if usable else None
        results.append(dict(line=L["name"], statistic=L["statistic"],
                            effect_slot=L["effect_slot"], bard_layer=L["bard_layer"],
                            chosen=chosen, n_usable=len(usable), n_members=len(avail),
                            alternatives=usable[1:4]))
    return results

# ---------------------------------------------------------------- main / report
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("classes", nargs="+")
    ap.add_argument("--level", type=int, default=60)
    ap.add_argument("--target", choices=["PLAYER", "PET"], default="PLAYER")
    ap.add_argument("--bard-in-group", action="store_true")
    ap.add_argument("--json")
    ap.add_argument("--buff-lines", default=os.path.join(BASE, "exports", "buff_lines.json"))
    ap.add_argument("--spells", default=os.path.join(BASE, "raw", "spells.json"))
    a = ap.parse_args()

    lines = json.load(open(a.buff_lines, encoding="utf-8"))
    clevels = load_spell_class_levels(a.spells)
    names = load_spell_names(a.spells)
    res = resolve(a.classes, a.level, a.target, a.bard_in_group, lines, clevels, names)

    covered = [r for r in res if r["chosen"]]
    self_only = sum(1 for r in covered if r["chosen"]["status"] == "SELF_CAST")
    item_only = sum(1 for r in covered if r["chosen"]["status"] == "ITEM")
    print(f"Build {'/'.join(c.upper() for c in a.classes)} @L{a.level} "
          f"target={a.target} bard_in_group={a.bard_in_group}")
    print(f"  lines for target : {len(res)}")
    print(f"  lines you can fill: {len(covered)}  (self-cast {self_only}, item {item_only})")
    print(f"  lines with NO build-available source: {len(res)-len(covered)}\n")
    print(f"  {'LINE':34} {'BEST':32} VALUE  SOURCE")
    for r in sorted(res, key=lambda r: (r['statistic'] or '', r['line'])):
        c = r["chosen"]
        if c:
            print(f"  {r['line'][:33]:34} {c['name'][:31]:32} {str(c['value'] or '?'):>5}  {c['why']}")
        else:
            print(f"  {r['line'][:33]:34} {'— none castable/obtainable —':32}")
    if a.json:
        json.dump(res, open(a.json, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
        print(f"\nwrote {a.json}")

if __name__ == "__main__":
    main()
