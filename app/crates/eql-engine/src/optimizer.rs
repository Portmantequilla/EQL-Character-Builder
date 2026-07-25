//! One-click gear optimization (user request 2026-07-21). Two profiles:
//!  * Optimal  — survival & longevity: AC, HP, stamina, resists, and an EVEN spread
//!               across all general attributes ("equality over all general stats").
//!  * MinMax   — maximum offense: weapon damage ratio, ATK, haste, and the build's
//!               offensive primary stats, with little regard for staying alive.
//!
//! Pure (no I/O): scores every class/race/era/level-legal item per paperdoll slot with
//! a profile- and role-weighted function and takes the best, then sockets a small,
//! bounded set of profile-appropriate Exaltation augments. Weights are domain-derived
//! (standard EQ gearing: tanks stack AC/HP/resists, DPS stacks ratio/ATK/haste) and
//! live here as named constants so they are easy to tune.
use crate::{augments, pet, stats, Snapshot};
use eql_data::{canonical_slot, era_allowed, BuildInput, Item, PAPERDOLL_SLOTS};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Optimal,
    MinMax,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Profile> {
        match s.to_ascii_uppercase().as_str() {
            "OPTIMAL" => Some(Profile::Optimal),
            "MINMAX" | "MIN_MAX" | "MIN-MAX" => Some(Profile::MinMax),
            _ => None,
        }
    }
}

/// Which role weightings apply, derived from the build's classes. A build can be more
/// than one (SHD/MNK/SHM is tank + melee + priest); every matching role turns on.
struct Roles {
    melee: bool,      // any class that swings a weapon / wants ATK
    priest: bool,     // CLR DRU SHM PAL RNG BST — WIS-based mana
    int_caster: bool, // NEC WIZ MAG ENC — INT-based mana
}

fn roles(classes: &[String]) -> Roles {
    let up: Vec<String> = classes.iter().map(|c| c.to_uppercase()).collect();
    let has = |set: &[&str]| up.iter().any(|c| set.contains(&c.as_str()));
    Roles {
        melee: has(&[
            "WAR", "PAL", "SHD", "RNG", "MNK", "ROG", "BER", "BST", "BRD",
        ]),
        // WIS classes (priests + hybrid priests); INT classes are the pure arcane set
        priest: has(&["CLR", "DRU", "SHM", "PAL", "RNG", "BST"]),
        int_caster: has(&["NEC", "WIZ", "MAG", "ENC"]),
    }
}

const RESISTS: [&str; 5] = ["SV MAGIC", "SV FIRE", "SV COLD", "SV POISON", "SV DISEASE"];

fn is_2h(it: &Item) -> bool {
    it.weapon_skill
        .as_deref()
        .is_some_and(|w| w.trim().to_ascii_uppercase().starts_with("2H"))
}

/// Profile score for one item in one slot. Higher = better for the profile. Weapon
/// slots additionally value the damage ratio; a caster-only build damps melee weights.
fn score(it: &Item, profile: Profile, r: &Roles) -> f64 {
    let g = |k: &str| it.stats.get(k).copied().unwrap_or(0) as f64;
    let ac = it.ac.unwrap_or(0) as f64;
    let hp = g("HP");
    let mana = g("MANA");
    let sta = g("STA");
    let (str_, dex, agi, cha) = (g("STR"), g("DEX"), g("AGI"), g("CHA"));
    let (int, wis) = (g("INT"), g("WIS"));
    let atk = g("ATK");
    let haste = it.haste_pct.unwrap_or(0) as f64;
    let resist: f64 = RESISTS.iter().map(|k| g(k)).sum();
    // damage ratio, normalized to be comparable to flat stats
    let ratio = match (it.dmg, it.atk_delay) {
        (Some(d), Some(dl)) if dl > 0 => d as f64 / dl as f64 * 100.0,
        _ => 0.0,
    };
    // the mana attribute the build actually casts from
    let mana_stat =
        if r.priest { wis } else { 0.0 } + if r.int_caster { int } else { 0.0 };
    let has_caster = r.priest || r.int_caster;
    // even-spread reward: sum of all seven general attributes (equality)
    let attr_sum = str_ + sta + agi + dex + wis + int + cha;

    match profile {
        Profile::Optimal => {
            let melee = if r.melee { 1.0 } else { 0.3 };
            3.0 * ac
                + 0.25 * hp
                + 2.0 * sta
                + 1.5 * resist
                + 0.8 * mana_stat
                + if has_caster { 0.12 * mana } else { 0.0 }
                + 0.5 * attr_sum // value every general stat (survival = well-rounded)
                + 0.4 * atk * melee
                + 0.5 * haste
                + 0.5 * ratio * melee
        }
        Profile::MinMax => {
            let melee = if r.melee { 1.0 } else { 0.2 };
            let caster = if has_caster { 1.0 } else { 0.2 };
            3.0 * ratio * melee
                + 2.0 * atk * melee
                + 2.0 * haste
                + 1.5 * str_ * melee
                + 1.2 * dex * melee
                + 0.3 * agi
                + 1.5 * mana_stat * caster
                + 0.05 * mana * caster
                + 0.2 * attr_sum
                + 0.3 * ac
                + 0.05 * hp
        }
    }
}

/// True when the item is legal for this build in the given canonical slot.
fn legal(it: &Item, want: &str, classes_up: &[String], build: &BuildInput, allow_epic: bool) -> bool {
    // Deliberately non-canonical entries must never be suggested to an ordinary build.
    // The optimizer reads the whole snapshot rather than the filtered picker list, so
    // without this they outscore every real item and get auto-equipped for everyone.
    if it.non_canonical && !eql_data::is_pickle_wizard(&build.classes) {
        return false;
    }
    // Epic quest weapons are opt-in: they come from long quest chains, not drops, so the
    // optimizer only suggests them when "Allow epic gear" is on. Hand-picking stays free.
    if it.is_epic && !allow_epic {
        return false;
    }
    let fits = want == "ANY" || it.slots.iter().any(|s| s == want);
    fits
        && (it.classes.iter().any(|c| c == "ALL")
            || it
                .classes
                .iter()
                .any(|ic| classes_up.iter().any(|c| c == ic)))
        && stats::race_legal(&it.races, build.race.as_deref())
        && era_allowed(it.era.as_deref(), &build.enabled_eras)
        && it.required_level.map_or(true, |rl| rl <= build.level as i64)
        // an optimizer must not hand you gear you can't wear: deity-locked items are
        // skipped (the build has no deity field), same for anything with no name
        && it.deities.is_empty()
        && !it.name.is_empty()
}

/// Best-scoring legal item for a slot, excluding already-used pageids and optionally a
/// weapon-hand filter. Returns (pageid, score).
fn best_for(
    snapshot: &Snapshot,
    want: &str,
    profile: Profile,
    r: &Roles,
    classes_up: &[String],
    build: &BuildInput,
    used: &BTreeSet<i64>,
    hand_filter: Option<bool>, // Some(true)=2H only, Some(false)=1H only, None=any
    allow_epic: bool,
) -> Option<(i64, f64)> {
    let mut best: Option<(i64, f64)> = None;
    for it in snapshot.items_by_id.values() {
        if used.contains(&it.pageid) {
            continue;
        }
        if let Some(want_2h) = hand_filter {
            if is_2h(it) != want_2h {
                continue;
            }
        }
        if !legal(it, want, classes_up, build, allow_epic) {
            continue;
        }
        let sc = score(it, profile, r);
        match best {
            Some((bp, bs)) if sc < bs || (sc == bs && it.pageid >= bp) => {}
            _ => best = Some((it.pageid, sc)),
        }
    }
    best
}

/// Optimize the worn player gear of `base` for `profile`. Keeps pet gear, spells,
/// buffs, race, classes, level, and era toggle; replaces every worn player slot.
/// Item upgrade tiers are deliberately left at 0 — those represent real in-game
/// investment, not something the planner should fabricate.
pub fn optimize_gear(
    snapshot: &Snapshot,
    base: &BuildInput,
    profile: Profile,
    allow_epic: bool,
) -> BuildInput {
    let mut build = base.clone();
    build.equipment.clear();
    build.equipment_tiers.retain(|k, _| k.starts_with("PET_"));
    build.augments.retain(|k, _| k.starts_with("PET_"));

    let r = roles(&build.classes);
    let classes_up: Vec<String> = build.classes.iter().map(|c| c.to_uppercase()).collect();
    let mut used: BTreeSet<i64> = BTreeSet::new();

    // ---- weapons first: compare best 2H against best 1H + best off-hand, because a
    // 2H costs you the secondary slot. Whichever total is higher wins.
    let p1h = best_for(snapshot, "PRIMARY", profile, &r, &classes_up, &build, &used, Some(false), allow_epic);
    let sec = p1h.map_or_else(
        || best_for(snapshot, "SECONDARY", profile, &r, &classes_up, &build, &used, None, allow_epic),
        |(pid, _)| {
            let mut u = used.clone();
            u.insert(pid);
            best_for(snapshot, "SECONDARY", profile, &r, &classes_up, &build, &u, None, allow_epic)
        },
    );
    let p2h = best_for(snapshot, "PRIMARY", profile, &r, &classes_up, &build, &used, Some(true), allow_epic);
    let combo_1h = p1h.map_or(0.0, |(_, s)| s) + sec.map_or(0.0, |(_, s)| s);
    match p2h {
        Some((pid, s2)) if s2 >= combo_1h && s2 > 0.0 => {
            build.equipment.insert("PRIMARY".into(), pid);
            used.insert(pid);
        }
        _ => {
            if let Some((pid, s)) = p1h {
                if s > 0.0 {
                    build.equipment.insert("PRIMARY".into(), pid);
                    used.insert(pid);
                }
            }
            if let Some((pid, s)) = sec {
                if s > 0.0 && !used.contains(&pid) {
                    build.equipment.insert("SECONDARY".into(), pid);
                    used.insert(pid);
                }
            }
        }
    }

    // ---- everything else, best-per-slot, no duplicate pageids ----
    for slot_key in PAPERDOLL_SLOTS {
        if slot_key == "PRIMARY" || slot_key == "SECONDARY" {
            continue;
        }
        let want = canonical_slot(slot_key);
        if let Some((pid, s)) =
            best_for(snapshot, want, profile, &r, &classes_up, &build, &used, None, allow_epic)
        {
            // ANY / AMMO / RANGE only get filled if the pick actually contributes
            let optional = matches!(want, "ANY" | "AMMO" | "RANGE");
            if s > 0.0 || !optional {
                build.equipment.insert(slot_key.to_string(), pid);
                used.insert(pid);
            }
        }
    }

    socket_exaltations(snapshot, &mut build, profile, &r, &classes_up);
    build
}

/// Focus family (strip the trailing tier numeral) so we socket at most one of each.
fn focus_family(effect: &str) -> String {
    effect
        .trim_end_matches(|c: char| c.is_ascii_digit() || c == 'I' || c == 'V' || c == ' ')
        .trim()
        .to_string()
}

/// Score an Exaltation source for the profile. WORN sources score on their real stat
/// delta (spell_stat_effects) through the same weighting as gear; FOCUS/PROC/CLICK
/// score by effect-name family (the standard focus names are stable).
fn aug_score(
    snapshot: &Snapshot,
    a: &eql_data::AugmentInfo,
    profile: Profile,
    r: &Roles,
) -> f64 {
    let name = a.effect_name.to_ascii_lowercase();
    let key = |needles: &[&str]| needles.iter().any(|n| name.contains(n));
    match a.socket.as_str() {
        "WORN" => {
            // build a synthetic item carrying the effect's flat stats, score it
            let mut probe = Item::default();
            if let Some(sid) = a.spell_id {
                for (stat, amt) in snapshot.spell_stat_effects.get(&sid).into_iter().flatten() {
                    if stat == "AC" {
                        probe.ac = Some(probe.ac.unwrap_or(0) + *amt as i64);
                    } else {
                        *probe.stats.entry(stat.clone()).or_default() += *amt as i64;
                    }
                }
            }
            score(&probe, profile, r)
        }
        "FOCUS" => match profile {
            Profile::Optimal => {
                if key(&["healing", "preservation", "mana"]) {
                    30.0
                } else if key(&["extended", "duration"]) {
                    12.0
                } else {
                    0.0
                }
            }
            Profile::MinMax => {
                if key(&["damage", "affliction", "burning", "fire", "ice", "magic"]) {
                    30.0
                } else if key(&["haste", "range"]) {
                    12.0
                } else {
                    0.0
                }
            }
        },
        "PROC" | "CLICK" => match profile {
            // procs/clicks are offense-flavored: only MinMax reaches for them, lightly
            Profile::MinMax if key(&["strike", "blast", "burst", "fire", "shock", "damage"]) => 10.0,
            _ => 0.0,
        },
        _ => 0.0,
    }
}

/// Socket a small, bounded set of profile-appropriate Exaltations into the freshly
/// chosen gear. Each source is used at most once; FOCUS families are deduped (one
/// Improved Damage is enough — it applies to every spell). Hand-only sources go only
/// into hand slots. Capped so this is a curated set, not spam.
fn socket_exaltations(
    snapshot: &Snapshot,
    build: &mut BuildInput,
    profile: Profile,
    r: &Roles,
    classes_up: &[String],
) {
    const MAX_SOCKETS: usize = 8;
    let mut catalog = augments::augment_catalog(snapshot);
    // legality: source class must intersect the build; in-era; level-appropriate
    catalog.retain(|a| {
        (a.classes.is_empty()
            || a.classes.iter().any(|c| c == "ALL")
            || a.classes.iter().any(|c| classes_up.iter().any(|b| b == c)))
            && era_allowed(a.era.as_deref(), &build.enabled_eras)
            && a.required_level.map_or(true, |rl| rl <= build.level as i64)
    });
    // rank by profile score, strongest first
    let mut ranked: Vec<(f64, eql_data::AugmentInfo)> = catalog
        .into_iter()
        .map(|a| (aug_score(snapshot, &a, profile, r), a))
        .filter(|(s, _)| *s > 0.0)
        .collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
        .then(a.1.source_pageid.cmp(&b.1.source_pageid)));

    // slot list in a stable order; hand sources only into PRIMARY/SECONDARY
    let slots: Vec<String> = PAPERDOLL_SLOTS
        .iter()
        .filter(|s| build.equipment.contains_key(**s))
        .map(|s| s.to_string())
        .collect();
    let is_hand = |slot: &str| matches!(slot, "PRIMARY" | "SECONDARY");
    let hand_only_source = |a: &eql_data::AugmentInfo| {
        !a.slots.is_empty()
            && a.slots
                .iter()
                .all(|s| matches!(s.as_str(), "PRIMARY" | "SECONDARY" | "RANGE"))
    };

    let mut used_sources: BTreeSet<i64> = BTreeSet::new();
    let mut used_focus_family: BTreeSet<String> = BTreeSet::new();
    let mut placed = 0usize;

    for (_, a) in ranked {
        if placed >= MAX_SOCKETS {
            break;
        }
        if used_sources.contains(&a.source_pageid) {
            continue;
        }
        if a.socket == "FOCUS" {
            let fam = focus_family(&a.effect_name);
            if used_focus_family.contains(&fam) {
                continue; // one of each focus family is plenty
            }
        }
        // find an equipped slot with this socket free, honoring the hand rule
        let target = slots.iter().find(|slot| {
            if hand_only_source(&a) && !is_hand(slot) {
                return false;
            }
            build
                .augments
                .get(*slot)
                .map_or(true, |m| !m.contains_key(&a.socket))
        });
        if let Some(slot) = target {
            build
                .augments
                .entry(slot.clone())
                .or_default()
                .insert(a.socket.clone(), a.source_pageid);
            used_sources.insert(a.source_pageid);
            if a.socket == "FOCUS" {
                used_focus_family.insert(focus_family(&a.effect_name));
            }
            placed += 1;
        }
    }
}

/// Suggest gear for the player's PET, scored by `profile` (Optimal = survival, the usual
/// choice for a pet since a dead pet does nothing). Fills only the pet's active-slot BUDGET
/// (base 4 + class bonuses) with the best items the PET's own class pool can wear — pets are
/// not race-restricted — greedily and without duplicate items. Player gear, spells, buffs,
/// and the pet summon are all left untouched; pet augment sockets are cleared (not auto-filled).
pub fn optimize_pet_gear(
    snapshot: &Snapshot,
    base: &BuildInput,
    profile: Profile,
    allow_epic: bool,
) -> BuildInput {
    let mut build = base.clone();

    // no pet summoned (or a zero budget) -> leave the build entirely untouched; the slot
    // budget is class/override-derived, not gear-derived, so resolving before clearing is safe.
    let Some(pet_block) = pet::resolve_pet(snapshot, &build) else { return build };
    let budget = pet_block.slot_count;
    if budget == 0 {
        return build;
    }

    // wipe only pet-side gear/tiers/augments; everything player-side is preserved
    build.pet_equipment.clear();
    build.equipment_tiers.retain(|k, _| !k.starts_with("PET_"));
    build.augments.retain(|k, _| !k.starts_with("PET_"));

    // the pet wears by ITS OWN classes; race restrictions don't apply to a summoned pet, so
    // score/legality run with race = None (race_legal returns true for None).
    let pet_classes = pet::pet_class_pool(snapshot, &build);
    let classes_up: Vec<String> = pet_classes.iter().map(|c| c.to_uppercase()).collect();
    let r = roles(&pet_classes);
    let mut legal_ctx = build.clone();
    legal_ctx.race = None;

    let pet_slots = eql_data::pet_paperdoll_slots();
    let mut used: BTreeSet<i64> = BTreeSet::new();
    // greedily fill the budget with the globally best available item across all empty pet
    // slots (no duplicate pageids). O(budget × slots × items) — trivial at these sizes.
    for _ in 0..budget {
        let mut best: Option<(usize, i64, f64)> = None; // (slot index, pageid, score)
        for (i, key) in pet_slots.iter().enumerate() {
            if build.pet_equipment.contains_key(key) {
                continue;
            }
            let canon = canonical_slot(key.strip_prefix("PET_").unwrap_or(key));
            if let Some((pid, s)) =
                best_for(snapshot, canon, profile, &r, &classes_up, &legal_ctx, &used, None, allow_epic)
            {
                if s > 0.0 && best.map_or(true, |(_, _, bs)| s > bs) {
                    best = Some((i, pid, s));
                }
            }
        }
        match best {
            Some((i, pid, _)) => {
                build.pet_equipment.insert(pet_slots[i].clone(), pid);
                used.insert(pid);
            }
            None => break, // nothing positive-scoring left to add
        }
    }
    build
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn mk(pid: i64, slots: &[&str], classes: &[&str], ac: i64, stats: &[(&str, i64)]) -> Item {
        Item {
            pageid: pid,
            name: format!("item{pid}"),
            slots: slots.iter().map(|s| s.to_string()).collect(),
            classes: classes.iter().map(|s| s.to_string()).collect(),
            ac: if ac != 0 { Some(ac) } else { None },
            stats: stats.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            ..Default::default()
        }
    }
    fn weapon(pid: i64, dmg: i64, delay: i64, skill: &str, stats: &[(&str, i64)]) -> Item {
        // 1H weapons list PRIMARY + SECONDARY (dual-wieldable); 2H list PRIMARY only
        let slots = if skill.to_uppercase().starts_with("2H") {
            vec!["PRIMARY".to_string()]
        } else {
            vec!["PRIMARY".to_string(), "SECONDARY".to_string()]
        };
        Item {
            pageid: pid,
            name: format!("wpn{pid}"),
            slots,
            classes: vec!["ALL".into()],
            dmg: Some(dmg),
            atk_delay: Some(delay),
            weapon_skill: Some(skill.into()),
            stats: stats.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            ..Default::default()
        }
    }

    fn snap(items: Vec<Item>) -> Snapshot {
        let mut s = Snapshot::default();
        for it in items {
            s.items_by_id.insert(it.pageid, it);
        }
        s
    }
    fn base(classes: &[&str]) -> BuildInput {
        BuildInput {
            level: 50,
            classes: classes.iter().map(|c| c.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn epics_are_opt_in() {
        // the epic outranks everything, but stays out of suggestions unless allowed
        let mut epic = weapon(1, 45, 20, "1H Slashing", &[("STR", 20), ("HP", 100)]);
        epic.is_epic = true;
        let plain = weapon(2, 10, 25, "1H Slashing", &[]);
        let s = snap(vec![epic, plain]);

        let default_opt = optimize_gear(&s, &base(&["WAR"]), Profile::MinMax, false);
        assert_eq!(
            default_opt.equipment.get("PRIMARY"),
            Some(&2),
            "default optimize must pick the dropped weapon, not the epic"
        );

        let allowed = optimize_gear(&s, &base(&["WAR"]), Profile::MinMax, true);
        assert_eq!(
            allowed.equipment.get("PRIMARY"),
            Some(&1),
            "with Allow epic gear on, the epic wins"
        );
    }

    #[test]
    fn optimal_prefers_survival_head() {
        // two HEAD items: a tanky one (AC + STA) vs a glassy one (STR + DEX)
        let tanky = mk(1, &["HEAD"], &["ALL"], 30, &[("STA", 15), ("SV MAGIC", 10)]);
        let glassy = mk(2, &["HEAD"], &["ALL"], 2, &[("STR", 25), ("DEX", 20)]);
        let s = snap(vec![tanky, glassy]);
        let opt = optimize_gear(&s, &base(&["WAR"]), Profile::Optimal, false);
        assert_eq!(opt.equipment.get("HEAD"), Some(&1), "Optimal takes the AC/STA head");
        let mm = optimize_gear(&s, &base(&["WAR"]), Profile::MinMax, false);
        assert_eq!(mm.equipment.get("HEAD"), Some(&2), "MinMax takes the STR/DEX head");
    }

    #[test]
    fn minmax_takes_2h_when_it_beats_dual_wield() {
        // a big 2H vs two weak 1H — MinMax melee should prefer the 2H and skip secondary
        let big2h = weapon(10, 60, 40, "2H Slashing", &[("STR", 20)]);
        let weak1 = weapon(11, 8, 30, "1H Slashing", &[]);
        let weak2 = weapon(12, 7, 30, "1H Slashing", &[]);
        let s = snap(vec![big2h, weak1, weak2]);
        let mm = optimize_gear(&s, &base(&["WAR"]), Profile::MinMax, false);
        assert_eq!(mm.equipment.get("PRIMARY"), Some(&10));
        assert_eq!(mm.equipment.get("SECONDARY"), None, "2H occupies both hands");
    }

    #[test]
    fn dual_wield_when_two_1h_beat_the_2h() {
        let ok2h = weapon(10, 20, 40, "2H Slashing", &[]);
        let good1 = weapon(11, 18, 20, "1H Slashing", &[]);
        let good2 = weapon(12, 17, 20, "1H Slashing", &[]);
        let s = snap(vec![ok2h, good1, good2]);
        let mm = optimize_gear(&s, &base(&["WAR"]), Profile::MinMax, false);
        assert!(mm.equipment.get("PRIMARY").is_some());
        assert!(mm.equipment.get("SECONDARY").is_some(), "two 1H beat the weak 2H");
        assert_ne!(mm.equipment["PRIMARY"], mm.equipment["SECONDARY"], "distinct weapons");
    }

    #[test]
    fn no_duplicate_pageids_across_finger_slots() {
        let r1 = mk(1, &["FINGER"], &["ALL"], 5, &[("STR", 10)]);
        let r2 = mk(2, &["FINGER"], &["ALL"], 4, &[("STR", 8)]);
        let s = snap(vec![r1, r2]);
        let opt = optimize_gear(&s, &base(&["WAR"]), Profile::Optimal, false);
        assert_eq!(opt.equipment.get("FINGER1"), Some(&1));
        assert_eq!(opt.equipment.get("FINGER2"), Some(&2), "second finger takes the next-best");
    }

    #[test]
    fn deity_and_class_locked_items_are_skipped() {
        let ok = mk(1, &["HEAD"], &["WAR"], 20, &[("STA", 10)]);
        let mut deitylocked = mk(2, &["HEAD"], &["WAR"], 99, &[("STA", 99)]);
        deitylocked.deities = vec!["Innoruuk".into()];
        let wrongclass = mk(3, &["HEAD"], &["CLR"], 99, &[("STA", 99)]);
        let s = snap(vec![ok, deitylocked, wrongclass]);
        let opt = optimize_gear(&s, &base(&["WAR"]), Profile::Optimal, false);
        assert_eq!(opt.equipment.get("HEAD"), Some(&1), "only the usable item is chosen");
    }

    #[test]
    fn keeps_pet_gear_and_clears_worn() {
        let head = mk(1, &["HEAD"], &["ALL"], 10, &[]);
        let s = snap(vec![head]);
        let mut b = base(&["WAR"]);
        b.equipment.insert("CHEST".into(), 999); // stale worn pick
        b.pet_equipment.insert("PET_PRIMARY".into(), 40);
        let mut tiers = BTreeMap::new();
        tiers.insert("PET_PRIMARY".to_string(), 3u32);
        tiers.insert("CHEST".to_string(), 5u32);
        b.equipment_tiers = tiers;
        let opt = optimize_gear(&s, &b, Profile::Optimal, false);
        assert_eq!(opt.pet_equipment.get("PET_PRIMARY"), Some(&40), "pet gear kept");
        assert_eq!(opt.equipment_tiers.get("PET_PRIMARY"), Some(&3u32), "pet tier kept");
        assert_eq!(opt.equipment_tiers.get("CHEST"), None, "worn tiers cleared");
        assert_eq!(opt.equipment.get("CHEST"), None, "stale worn pick replaced");
    }
}
