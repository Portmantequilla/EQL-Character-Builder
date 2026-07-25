"""Golden tests for the reference buff resolver — the executable spec the Rust M2
engine is validated against (plan risk 8). Run: python scripts/test_resolve_buffs.py"""
import os, json, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import resolve_buffs as R

BASE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LINES  = json.load(open(os.path.join(BASE,"exports","buff_lines.json"), encoding="utf-8"))
CLEV   = R.load_spell_class_levels(os.path.join(BASE,"raw","spells.json"))
NAMES  = R.load_spell_names(os.path.join(BASE,"raw","spells.json"))

def plan(build, level=60, target="PLAYER", bard=False):
    res = R.resolve(build, level, target, bard, LINES, CLEV, NAMES)
    return {r["line"]: r for r in res}

def chosen(p, line):
    c = p[line]["chosen"]; return (c["name"], c["value"], c["status"]) if c else None

def run():
    p = plan(["SHD","MNK","SHM"])
    cases = [
        # (assertion desc, actual, expected)
        ("Strength(Primary)=Maniacal Strength +68 self-cast",
         chosen(p,"Strength (Primary)"), ("Maniacal Strength", 68.0, "SELF_CAST")),
        ("Strength(Power)=Focus of Spirit +67 self-cast",
         chosen(p,"Strength (Power)"), ("Focus of Spirit", 67.0, "SELF_CAST")),
        # a bard-only line is NOT self-fillable by SHD/MNK/SHM (no bard, no item)
        ("Strength(Anthem) unfillable without bard",
         p["Strength (Anthem)"]["chosen"], None),
        # level gating: SHM gets Strength(+67) at L56, upgrades to Maniacal(+68) at L57
        ("Level gating L56 -> Strength +67",
         chosen(plan(["SHM"],level=56),"Strength (Primary)"), ("Strength", 67.0, "SELF_CAST")),
        ("Level gating L57 -> Maniacal Strength +68",
         chosen(plan(["SHM"],level=57),"Strength (Primary)"), ("Maniacal Strength", 68.0, "SELF_CAST")),
    ]
    ok = True
    for desc, got, exp in cases:
        good = (got == exp)
        print(("PASS" if good else "FAIL"), "-", desc, "| got:", got)
        ok = ok and good
    # coverage sanity
    covered = sum(1 for r in plan(["SHD","MNK","SHM"]).values() if r["chosen"])
    print(f"\ncoverage: {covered} lines fillable for SHD/MNK/SHM @L60")
    assert covered >= 60, "expected >=60 fillable lines"
    print("ALL GOLDEN CHECKS", "PASSED" if ok else "FAILED")
    sys.exit(0 if ok else 1)

if __name__ == "__main__":
    run()
