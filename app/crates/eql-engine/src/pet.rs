//! Pet resolution (plan §13-§17): summon selection, level calculation, intrinsic class
//! pair, the equip class pool, the give-item pet paperdoll (23 wells; the class-combo
//! slot budget = base + summed class bonuses limits how many filled wells are ACTIVE)
//! with per-item badges, and pet-target buff lines.
use crate::{resolver, Snapshot};
use eql_data::{BuildInput, Item, PetBlock, PetGearSlot, PetWeapon, Target};
use std::collections::BTreeMap;

/// A two-handed weapon (weapon_skill "2H Slashing/Blunt/Piercing") — occupies both hands.
fn is_2h(it: &Item) -> bool {
    it.weapon_skill
        .as_deref()
        .map(|s| s.trim().to_ascii_uppercase().starts_with("2H"))
        .unwrap_or(false)
}
/// Item is placeable in the given hand-slot ("PRIMARY"/"SECONDARY"). Empty slot data
/// (a wiki gap) is treated permissively so a melee weapon isn't wrongly excluded.
fn goes_hand(it: &Item, hand: &str) -> bool {
    it.slots.iter().any(|s| s.eq_ignore_ascii_case(hand))
        || it.slot.as_deref().is_some_and(|s| s.to_ascii_uppercase().contains(hand))
}
fn goes_secondary(it: &Item) -> bool {
    goes_hand(it, "SECONDARY")
}
/// A melee/hand weapon (has damage AND lives in a hand). Excludes bows/thrown, which
/// carry damage but occupy RANGE/AMMO and never take a primary/secondary hand.
fn is_weapon(it: &Item) -> bool {
    it.dmg.is_some()
        && (goes_hand(it, "PRIMARY") || goes_secondary(it) || (it.slots.is_empty() && it.slot.is_none()))
}
/// A non-weapon that occupies the off-hand (shield or secondary-only item).
fn is_offhand(it: &Item) -> bool {
    it.dmg.is_none() && goes_secondary(it)
}
/// True when the pet is given this item in a hand slot at all (weapon or off-hand).
fn is_hand_item(it: &Item) -> bool {
    is_weapon(it) || is_offhand(it)
}

/// The classes whose gear the pet can use: the PET's OWN classes (user-verified in
/// game 2026-07-21 — a WAR/BST Spirit of Kashek uses WAR gear regardless of the
/// owner's trio; replaces the old intrinsic-UNION-owner reading). When the summon's
/// classes are unknown in the wiki data (or no summon is selected), fall back to the
/// owner's classes rather than rejecting everything. Also the class rule for
/// Exaltation augments socketed into pet gear (augments.rs).
pub fn pet_class_pool(snapshot: &Snapshot, build: &BuildInput) -> Vec<String> {
    let intrinsic: Vec<String> = build
        .pet_summon_spell_id
        .and_then(|sid| snapshot.pet_summons.get(&sid))
        .and_then(|s| s.pet_classes.as_deref())
        .map(|pc| pc.split('/').map(|s| s.trim().to_uppercase()).collect())
        .unwrap_or_default();
    if intrinsic.is_empty() {
        build.classes.iter().map(|c| c.to_uppercase()).collect()
    } else {
        intrinsic
    }
}

/// Summons castable by the build at its level, strongest (highest learn level) first.
/// Locked class slots (third unlocks at 11) contribute no summon spells.
pub fn available_summons(snapshot: &Snapshot, build: &BuildInput) -> Vec<i64> {
    let classes = crate::unlocked_classes(snapshot, build);
    let mut v: Vec<(u32, i64)> = snapshot
        .pet_summons
        .iter()
        .filter_map(|(sid, info)| {
            info.class_levels
                .iter()
                .filter(|(c, lv)| classes.contains(c) && **lv <= build.level)
                .map(|(_, lv)| *lv)
                .min()
                .map(|lv| (lv, *sid))
        })
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v.into_iter().map(|(_, sid)| sid).collect()
}

/// Resolve the selected pet: level formula MIN(base + tier, character level - 1)
/// (Pet Guide, WIKI_CONFIRMED); base may be unknown (NEEDS_INGAME_TEST -> None).
pub fn resolve_pet(snapshot: &Snapshot, build: &BuildInput) -> Option<PetBlock> {
    let sid = build.pet_summon_spell_id?;
    let summon = snapshot.pet_summons.get(&sid)?.clone();
    // saved-but-inactive (plan §18): keep the selection, flag when no longer castable.
    // Locked class slots (third unlocks at 11) can't cast their summons yet.
    let classes = crate::unlocked_classes(snapshot, build);
    let castable_at = summon
        .class_levels
        .iter()
        .filter(|(c, _)| classes.contains(c))
        .map(|(_, lv)| *lv)
        .min();
    let valid = castable_at.is_some_and(|lv| lv <= build.level);
    // effective tier: the summon SPELL's upgrade tier drives the pet (spellbook
    // steppers); pet_summon_tier kept as a legacy input — the max applies
    let effective_tier = build
        .pet_summon_tier
        .max(build.spell_tiers.get(&sid).copied().unwrap_or(0))
        .min(10);
    let calculated_level = summon.base_pet_level.map(|base| {
        // MIN(base + tier, level - 1), floored at 1 (a level-0 pet is not meaningful;
        // the level-1-summoner edge is on the §8.2 in-game checklist)
        (base + effective_tier as i64).min(build.level as i64 - 1).max(1)
    });
    // Per-level scaling — official 7/7/2026 notes: each tier ATTEMPTS +1 pet level
    // (capped at owner level - 1), and ONLY levels actually gained grant stats:
    // +6% HP, +1 base damage, +5 skill points per gained level. Ranks eaten by the
    // cap grant NOTHING (user research workbook 2026-07-16; replaces the old
    // scale-by-raw-tier reading). Base level unknown -> fall back to the tier, noted.
    let levels_gained: Option<u32> = summon.base_pet_level.map(|base| {
        calculated_level
            .map(|lvl| (lvl - base).max(0) as u32)
            .unwrap_or(effective_tier)
            .min(effective_tier)
    });
    let gain = levels_gained.unwrap_or(effective_tier);
    let tier_capped = levels_gained.is_some_and(|g| g < effective_tier);
    let pet_hp_scaled = summon
        .pet_hp
        .map(|hp| ((hp as f64) * (1.0 + 0.06 * gain as f64)).floor() as i64);
    let pet_max_hit_scaled = summon.pet_max_hit.map(|h| h + gain as i64);
    let skill_point_bonus = 5 * gain;
    let intrinsic: Vec<String> = summon
        .pet_classes
        .as_deref()
        .map(|pc| pc.split('/').map(|s| s.trim().to_uppercase()).collect())
        .unwrap_or_default();
    let pool = pet_class_pool(snapshot, build);
    let mut notes = vec![
        if intrinsic.is_empty() {
            "pet item pool: this summon's own classes are unknown in the wiki data — \
             falling back to the owner's classes until measured"
                .to_string()
        } else {
            "pet item pool = the PET's own classes, not the owner's (user-verified in \
             game 2026-07-21: WAR/BST pet uses WAR gear the owner trio can't)"
                .to_string()
        },
        "pets always respect proc level requirements (Pet Guide, WIKI_CONFIRMED)".to_string(),
        "only ACTUALLY GAINED levels grant stats: +6% HP, +1 base damage, +5 skill \
         points each (official 7/7 notes) — tier ranks eaten by the player-1 cap grant \
         nothing"
            .to_string(),
    ];
    if tier_capped {
        notes.push(format!(
            "tier {} attempted +{} levels but only +{} fit under player level - 1 — \
             the capped ranks grant no stats",
            effective_tier, effective_tier, gain
        ));
    }
    if levels_gained.is_none() && effective_tier > 0 {
        notes.push(
            "base pet level unknown: stat scaling falls back to the raw tier (may \
             overstate a capped pet)"
                .to_string(),
        );
    }
    if let Some(conf) = &summon.estimate_confidence {
        notes.push(format!(
            "level/HP/hit partly from the research workbook ({conf}) — validate in game"
        ));
    }
    if !valid {
        notes.insert(0, match castable_at {
            Some(lv) => format!(
                "SAVED_INACTIVE: summon not castable by this build until level {lv} — \
                 selection kept, pet contributes nothing"
            ),
            None => "SAVED_INACTIVE: summon not castable by any selected class — \
                     selection kept, pet contributes nothing".to_string(),
        });
    }
    if calculated_level.is_none() {
        notes.push(
            "base pet level unknown for this summon (wiki silent) — enter it via overrides \
             when measured in game (checklist V1)"
                .to_string(),
        );
    }
    // ---- pet inventory (plan §17): base slots + the SUM of every unlocked class's
    // bonus. VERIFIED_INGAME 2026-07-20: a level-1 MAG/BST shows 10 slots (4+3+3),
    // which refutes the earlier max-wins reading; SHD/MNK/SHM = 5 (4+0+0+1) fits both
    // models and stays correct. Max possible: MAG/BST/NEC = 12.
    let rule = &snapshot.pet_slot_rule;
    // zero-bonus classes (e.g. SHD: 0) are never CREDITED in the label — a
    // "+ SHD+0" would claim a bonus that doesn't exist
    let contributors: Vec<(String, usize)> = classes
        .iter()
        .filter_map(|c| rule.bonus.get(c).map(|b| (c.clone(), *b)))
        .filter(|(_, b)| *b > 0)
        .collect();
    let bonus: usize = contributors.iter().map(|(_, b)| *b).sum();
    let slot_bonus_class = if contributors.is_empty() {
        None
    } else {
        Some(
            contributors
                .iter()
                .map(|(c, b)| format!("{c}+{b}"))
                .collect::<Vec<_>>()
                .join(" "),
        )
    };
    let default_slot_count = rule.slots_base + bonus;
    // a manual override (what the user sees in game) still wins — clamped to a sane
    // range so the UI can't request an absurd grid.
    let slot_count_overridden = build.pet_slot_override.is_some_and(|n| n >= 1);
    let slot_count = build
        .pet_slot_override
        .filter(|&n| n >= 1)
        .map(|n| (n as usize).min(eql_data::PET_SLOT_MAX))
        .unwrap_or(default_slot_count);

    // ---- per-slot validation (plan §14/§15): the pet paperdoll (23 wells, in-game
    // row order). Filled wells consume the class-combo slot budget IN ROW ORDER; wells
    // over the budget go OVER_CAP (red, contribute nothing). Class pool gates equip;
    // proc level gates ONLY the proc ("weapon stats active, proc inactive until L N").
    let filled_count = eql_data::pet_paperdoll_slots()
        .iter()
        .filter(|k| build.pet_equipment.contains_key(*k))
        .count();
    let mut budget = slot_count;
    let mut gear = Vec::new();
    let mut gear_totals: BTreeMap<String, i64> = BTreeMap::new();
    for key in eql_data::pet_paperdoll_slots() {
        let Some(pid) = build.pet_equipment.get(&key) else {
            // empty well: green while the budget still has room for one more item
            gear.push(PetGearSlot { slot: key, item_pageid: None, item_name: None,
                icon_id: None, badge: "EMPTY".into(), reason: None,
                active: filled_count < slot_count });
            continue;
        };
        if budget == 0 {
            let name = snapshot.items_by_id.get(pid).map(|i| i.name.clone());
            let icon = snapshot.items_by_id.get(pid).and_then(|i| i.icon_id);
            gear.push(PetGearSlot { slot: key, item_pageid: Some(*pid), item_name: name,
                icon_id: icon, badge: "OVER_CAP".into(),
                reason: Some(format!(
                    "over the {slot_count}-slot budget for this class combo — \
                     contributes nothing")),
                active: false });
            continue;
        }
        budget -= 1;
        let Some(item) = snapshot.items_by_id.get(pid) else {
            gear.push(PetGearSlot { slot: key, item_pageid: Some(*pid), item_name: None,
                icon_id: None, badge: "INVALID_CLASS".into(),
                reason: Some("item missing from wiki data".into()), active: true });
            continue;
        };
        let class_ok = item.classes.iter().any(|c| c == "ALL")
            || item.classes.iter().any(|ic| pool.iter().any(|p| p.eq_ignore_ascii_case(ic)));
        if !class_ok {
            gear.push(PetGearSlot { slot: key, item_pageid: Some(*pid),
                item_name: Some(item.name.clone()), icon_id: item.icon_id,
                badge: "INVALID_CLASS".into(),
                reason: Some(format!("classes {} outside the pet pool {}",
                                     item.classes.join("/"), pool.join("/"))),
                active: true });
            continue;
        }
        if !eql_data::era_allowed(item.era.as_deref(), &build.enabled_eras) {
            gear.push(PetGearSlot { slot: key, item_pageid: Some(*pid),
                item_name: Some(item.name.clone()), icon_id: item.icon_id,
                badge: "OUT_OF_ERA".into(),
                reason: Some(format!("out of enabled expansions ({})",
                                     item.era.as_deref().unwrap_or("?"))),
                active: true });
            continue;
        }
        // stats count once class-legal (plan §15 example); upgrade tiers apply via the
        // exact community rule (item_tier_stat)
        let tier = build.equipment_tiers.get(&key).copied().unwrap_or(0).min(10);
        for (k, v) in &item.stats {
            *gear_totals.entry(k.clone()).or_default() += eql_data::item_tier_stat(*v, tier);
        }
        if let Some(ac) = item.ac {
            *gear_totals.entry("AC".into()).or_default() += eql_data::item_tier_stat(ac, tier);
        }
        // proc gate: pets ALWAYS respect proc level requirements (WIKI_CONFIRMED)
        let inactive_proc = snapshot
            .item_effects
            .get(pid)
            .into_iter()
            .flatten()
            .filter(|e| e.activation_type == "PROC" || e.activation_type == "WORN")
            .find(|e| match (e.required_level, calculated_level) {
                (Some(req), Some(pl)) => pl < req,
                (Some(_), None) => true, // unknown pet level: be honest, flag it
                _ => false,
            });
        match inactive_proc {
            Some(e) => gear.push(PetGearSlot {
                slot: key, item_pageid: Some(*pid), item_name: Some(item.name.clone()),
                icon_id: item.icon_id, badge: "PROC_INACTIVE".into(),
                reason: Some(match calculated_level {
                    Some(_) => format!("{} inactive: pet requires level {}",
                                       e.effect_name, e.required_level.unwrap_or(0)),
                    None => format!("{} gate unknown (pet level unknown)", e.effect_name),
                }),
                active: true,
            }),
            None => gear.push(PetGearSlot {
                slot: key, item_pageid: Some(*pid), item_name: Some(item.name.clone()),
                icon_id: item.icon_id, badge: "FULLY_ACTIVE".into(), reason: None,
                active: true,
            }),
        }
    }

    // ---- weapon hand rule (1×2H / 2×1H / 1H+shield). Only actually-equipped gear
    // (class-legal, in-era, within budget) counts; proc-inactive still wields the weapon
    // (only the proc sleeps). Only the hand-capable wells participate — PRIMARY and
    // SECONDARY first (hand assignment follows well placement), then the ANY wells
    // (give-item overflow); a weapon parked in an armor well never wields.
    let hand_rank = |slot: &str| match slot {
        "PET_PRIMARY" => Some(0),
        "PET_SECONDARY" => Some(1),
        "PET_ANY1" => Some(2),
        "PET_ANY2" => Some(3),
        _ => None,
    };
    let mut hand_items: Vec<(usize, String, &Item)> = gear
        .iter()
        .filter(|g| g.badge == "FULLY_ACTIVE" || g.badge == "PROC_INACTIVE")
        .filter_map(|g| {
            let rank = hand_rank(&g.slot)?;
            g.item_pageid
                .and_then(|pid| snapshot.items_by_id.get(&pid))
                .filter(|it| is_hand_item(it))
                .map(|it| (rank, g.slot.clone(), it))
        })
        .collect();
    hand_items.sort_by_key(|(rank, _, _)| *rank);
    let hand_items: Vec<(String, &Item)> =
        hand_items.into_iter().map(|(_, s, it)| (s, it)).collect();
    let (weapon_config, mut weapon_warnings, weapon_summary) = assign_pet_weapons(&hand_items);
    // dual wield begins at pet level 5 (Pet Guide / user research workbook): warn when a
    // second one-hander is wielded below that — advisory, the loadout itself stays legal
    let dual_wielding = weapon_config
        .iter()
        .filter(|w| w.active && w.category == "1H")
        .count()
        >= 2;
    if dual_wielding && calculated_level.is_some_and(|l| l < 5) {
        weapon_warnings.push(format!(
            "dual wield begins at pet level 5 — this pet is level {} and may not use \
             the second weapon yet",
            calculated_level.unwrap_or(0)
        ));
    }

    // pet-target buff lines resolved at the PET (owner-cast candidates; plan §16 slice);
    // the owner's spell tiers apply (Burnout at tier N scales like any owner cast)
    let spell_tier_pct: f64 = snapshot
        .formulas
        .get("spell_tier_scaling_pct")
        .and_then(|v| v.parse().ok())
        .unwrap_or(6.0);
    let cons = crate::build_constraints(snapshot, build);
    let buff_lines = resolver::resolve_buff_lines_full(
        &snapshot.buff_lines,
        &snapshot.class_levels,
        &snapshot.spell_names,
        &build.classes,
        build.level,
        Target::Pet,
        build.bard_in_group,
        &build.spell_tiers,
        spell_tier_pct,
        &cons,
    )
    .into_iter()
    .map(|f| f.res)
    .collect();
    Some(PetBlock {
        summon,
        valid,
        becomes_valid_at: castable_at.filter(|lv| *lv > build.level),
        calculated_level,
        effective_tier,
        levels_gained,
        pet_hp_scaled,
        pet_max_hit_scaled,
        skill_point_bonus,
        tier_capped,
        intrinsic_classes: intrinsic,
        equip_class_pool: pool,
        slot_count,
        default_slot_count,
        slot_count_overridden,
        slot_bonus_class,
        gear,
        gear_totals,
        weapon_config,
        weapon_summary,
        weapon_warnings,
        buff_lines,
        notes,
    })
}

/// Apply the pet weapon hand rule to the items the pet was given in hand slots, in the
/// order given. Weapons claim hands first (a 2H takes both), then shields/off-hands fill
/// the remaining hand. Returns (per-item config, rule violations, one-line summary).
/// Pure — takes only the ordered (slot, item) list, so it is unit-tested directly.
fn assign_pet_weapons(hand_items: &[(String, &Item)]) -> (Vec<PetWeapon>, Vec<String>, Option<String>) {
    let (mut config, mut warnings) = (Vec::new(), Vec::new());
    let (mut primary_taken, mut secondary_taken, mut has_2h) = (false, false, false);
    // pass 1: weapons claim hands
    for (slot, it) in hand_items.iter().filter(|(_, it)| is_weapon(it)) {
        if is_2h(it) {
            if !primary_taken && !secondary_taken {
                primary_taken = true; secondary_taken = true; has_2h = true;
                config.push(weapon(slot, it, "2H", Some("PRIMARY"), true, None));
            } else {
                warnings.push(format!("{} (2H) can't be wielded — the pet's hands are already in use", it.name));
                config.push(weapon(slot, it, "2H", None, false, Some("hands already in use")));
            }
        } else if !primary_taken {
            primary_taken = true;
            config.push(weapon(slot, it, "1H", Some("PRIMARY"), true, None));
        } else if !secondary_taken {
            secondary_taken = true;
            config.push(weapon(slot, it, "1H", Some("SECONDARY"), true, None));
        } else {
            let why = if has_2h { "a two-hander occupies both hands" } else { "both hands are already in use" };
            warnings.push(format!("{} can't be dual-wielded — {why}", it.name));
            config.push(weapon(slot, it, "1H", None, false, Some(why)));
        }
    }
    // pass 2: shields / off-hands take the remaining hand
    for (slot, it) in hand_items.iter().filter(|(_, it)| !is_weapon(it)) {
        let cat = if it.ac.is_some() { "SHIELD" } else { "OFFHAND" };
        if has_2h {
            warnings.push(format!("off-hand {} is unused — a two-handed weapon occupies both hands", it.name));
            config.push(weapon(slot, it, cat, None, false, Some("two-hander occupies both hands")));
        } else if !secondary_taken {
            secondary_taken = true;
            let note = (!primary_taken).then_some("off-hand with no main weapon");
            config.push(weapon(slot, it, cat, Some("SECONDARY"), true, note));
        } else {
            warnings.push(format!("off-hand {} is unused — the off-hand is already occupied", it.name));
            config.push(weapon(slot, it, cat, None, false, Some("off-hand already occupied")));
        }
    }
    let prim = config.iter().find(|w| w.active && w.hand.as_deref() == Some("PRIMARY"));
    let sec = config.iter().find(|w| w.active && w.hand.as_deref() == Some("SECONDARY"));
    let summary = match (prim, sec) {
        (Some(p), _) if p.category == "2H" => Some(format!("wielding {} (two-handed)", p.item_name)),
        (Some(p), Some(s)) if s.category == "1H" => Some(format!("dual-wielding {} + {}", p.item_name, s.item_name)),
        (Some(p), Some(s)) => Some(format!("{} + {} ({})", p.item_name, s.item_name, s.category.to_lowercase())),
        (Some(p), None) => Some(format!("wielding {} (off-hand empty)", p.item_name)),
        (None, Some(s)) => Some(format!("holding {} (no main weapon)", s.item_name)),
        (None, None) => None,
    };
    (config, warnings, summary)
}

fn weapon(slot: &str, it: &Item, cat: &str, hand: Option<&str>, active: bool, note: Option<&str>) -> PetWeapon {
    PetWeapon {
        slot: slot.to_string(),
        item_name: it.name.clone(),
        category: cat.to_string(),
        hand: hand.map(String::from),
        active,
        note: note.map(String::from),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(name: &str, dmg: Option<i64>, skill: Option<&str>, ac: Option<i64>, secondary: bool) -> Item {
        Item {
            name: name.into(),
            dmg,
            weapon_skill: skill.map(String::from),
            ac,
            slots: if secondary { vec!["SECONDARY".into()] } else { vec!["PRIMARY".into()] },
            ..Default::default()
        }
    }
    fn oneh(name: &str) -> Item { mk(name, Some(10), Some("1H Slashing"), None, false) }
    fn twoh(name: &str) -> Item { mk(name, Some(20), Some("2H Slashing"), None, false) }
    fn shield(name: &str) -> Item { mk(name, None, None, Some(15), true) }
    fn assign<'a>(items: &'a [(String, &'a Item)]) -> (Vec<PetWeapon>, Vec<String>, Option<String>) {
        assign_pet_weapons(items)
    }
    fn slots<'a>(items: &'a [Item]) -> Vec<(String, &'a Item)> {
        items.iter().enumerate().map(|(i, it)| (format!("PET_{}", i + 1), it)).collect()
    }

    #[test]
    fn two_1h_dual_wield_ok() {
        let items = [oneh("Rusty Sword"), oneh("Rusty Dagger")];
        let (cfg, warn, sum) = assign(&slots(&items));
        assert!(warn.is_empty());
        assert_eq!(cfg[0].hand.as_deref(), Some("PRIMARY"));
        assert_eq!(cfg[1].hand.as_deref(), Some("SECONDARY"));
        assert!(cfg.iter().all(|w| w.active));
        assert_eq!(sum.as_deref(), Some("dual-wielding Rusty Sword + Rusty Dagger"));
    }

    #[test]
    fn single_2h_ok_and_occupies_both_hands() {
        let items = [twoh("Great Axe")];
        let (cfg, warn, sum) = assign(&slots(&items));
        assert!(warn.is_empty());
        assert_eq!(cfg[0].category, "2H");
        assert!(cfg[0].active);
        assert_eq!(sum.as_deref(), Some("wielding Great Axe (two-handed)"));
    }

    #[test]
    fn one_1h_plus_shield_ok() {
        let items = [oneh("Short Sword"), shield("Kite Shield")];
        let (cfg, warn, sum) = assign(&slots(&items));
        assert!(warn.is_empty());
        assert_eq!(cfg[1].category, "SHIELD");
        assert_eq!(cfg[1].hand.as_deref(), Some("SECONDARY"));
        assert_eq!(sum.as_deref(), Some("Short Sword + Kite Shield (shield)"));
    }

    #[test]
    fn two_hander_plus_offhand_warns_and_offhand_inactive() {
        let items = [twoh("Great Axe"), shield("Kite Shield")];
        let (cfg, warn, _) = assign(&slots(&items));
        assert_eq!(warn.len(), 1);
        assert!(warn[0].contains("two-handed"));
        let sh = cfg.iter().find(|w| w.category == "SHIELD").unwrap();
        assert!(!sh.active);
        assert!(sh.hand.is_none());
    }

    #[test]
    fn three_weapons_third_is_inactive() {
        let items = [oneh("A"), oneh("B"), oneh("C")];
        let (cfg, warn, _) = assign(&slots(&items));
        assert_eq!(warn.len(), 1);
        assert_eq!(cfg.iter().filter(|w| w.active).count(), 2);
        assert!(!cfg[2].active);
    }

    #[test]
    fn two_2h_second_cannot_be_wielded() {
        let items = [twoh("Axe1"), twoh("Axe2")];
        let (cfg, warn, _) = assign(&slots(&items));
        assert_eq!(warn.len(), 1);
        assert!(cfg[0].active && !cfg[1].active);
    }

    #[test]
    fn weapon_first_then_shield_regardless_of_input_order() {
        // shield given BEFORE the weapon: the two-pass must still put the weapon in
        // primary and the shield in secondary (no false "no main weapon" note)
        let items = [shield("Kite Shield"), oneh("Short Sword")];
        let (cfg, warn, sum) = assign(&slots(&items));
        assert!(warn.is_empty());
        let sword = cfg.iter().find(|w| w.item_name == "Short Sword").unwrap();
        let sh = cfg.iter().find(|w| w.item_name == "Kite Shield").unwrap();
        assert_eq!(sword.hand.as_deref(), Some("PRIMARY"));
        assert_eq!(sh.hand.as_deref(), Some("SECONDARY"));
        assert!(sh.note.is_none());
        assert_eq!(sum.as_deref(), Some("Short Sword + Kite Shield (shield)"));
    }

    #[test]
    fn classifiers() {
        assert!(is_2h(&twoh("x")));
        assert!(!is_2h(&oneh("x")));
        assert!(is_weapon(&oneh("x")) && is_weapon(&twoh("x")));
        assert!(!is_weapon(&shield("x")));
        assert!(is_offhand(&shield("x")));
        assert!(!is_offhand(&oneh("x")));
        assert!(is_hand_item(&oneh("x")) && is_hand_item(&shield("x")));
    }

    #[test]
    fn bow_is_not_a_hand_weapon() {
        // a bow has damage but occupies RANGE — it must not claim a primary/secondary hand
        let bow = Item {
            name: "Short Bow".into(), dmg: Some(8), weapon_skill: Some("Archery".into()),
            slots: vec!["RANGE".into()], ..Default::default()
        };
        assert!(!is_weapon(&bow));
        assert!(!is_hand_item(&bow));
        // given alongside a real weapon, the bow is ignored by the hand rule
        let sword = oneh("Sword");
        let items = [(String::from("PET_1"), &bow), (String::from("PET_2"), &sword)];
        let (cfg, warn, _) = assign_pet_weapons(&items[1..]); // caller pre-filters non-hand items
        assert!(warn.is_empty());
        assert_eq!(cfg.len(), 1);
        assert_eq!(cfg[0].item_name, "Sword");
    }
}
