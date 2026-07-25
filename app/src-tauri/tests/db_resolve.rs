//! DB-fed integration test: build the real Snapshot from db/eql.db and require the
//! engine to reproduce the oracle acceptance numbers (docs/M2-handoff.md):
//! SHD/MNK/SHM @L60 -> 112 player lines, 70 fillable (20 self-cast, 50 item).
//! Also exercises the full resolve_build pipeline + the seeded chooser end-to-end.
use eql_builder_lib::db;
use eql_data::{BuildInput, MemberStatus, Target};
use eql_engine::{resolve_buff_lines, resolve_build};
use std::path::PathBuf;

fn point_at_repo_db() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("db").join("eql.db");
    assert!(p.exists(), "expected wiki db at {p:?}");
    std::env::set_var("EQL_WIKI_DB", &p);
    // keep test builds.db out of %LOCALAPPDATA%
    let tmp = std::env::temp_dir().join("eql_test_builds.db");
    let _ = std::fs::remove_file(&tmp);
    std::env::set_var("EQL_BUILDS_DB", tmp);
}

#[test]
fn db_fed_pipeline() {
    point_at_repo_db();
    let snap = db::snapshot();

    // ---- resolver acceptance numbers (M2 handoff DoD)
    let build_classes = vec!["SHD".to_string(), "MNK".to_string(), "SHM".to_string()];
    let res = resolve_buff_lines(
        &snap.buff_lines, &snap.class_levels, &snap.spell_names,
        &build_classes, 60, Target::Player, false,
    );
    assert_eq!(res.len(), 112, "player lines");
    let covered: Vec<_> = res.iter().filter(|r| r.chosen.is_some()).collect();
    let self_cast = covered.iter()
        .filter(|r| r.chosen.as_ref().unwrap().status == MemberStatus::SelfCast).count();
    // Sourcing spec 2026-07-17: item/consumable sources NEVER auto-apply (they were 50
    // of the old 70 "fillable" lines) — the default plan is what the character can do
    // ALONE. 31 lines now self-fill: the old 20 self-cast picks plus 11 lines where an
    // item member used to outrank the self-cast member and now falls back to it.
    assert_eq!(covered.len(), 31, "self-fillable (items are opt-in now)");
    assert_eq!(self_cast, 31, "every default pick is the character's own cast");

    let chosen = |line: &str| -> Option<(String, f64)> {
        res.iter().find(|r| r.line == line)
            .and_then(|r| r.chosen.as_ref())
            .map(|c| (c.name.clone(), c.value.unwrap()))
    };
    assert_eq!(chosen("Strength (Primary)"), Some(("Maniacal Strength".into(), 68.0)));
    assert_eq!(chosen("Strength (Power)"), Some(("Focus of Spirit".into(), 67.0)));
    assert_eq!(chosen("Strength (Anthem)"), None);

    // ---- full pipeline: resolve_build with equipment
    // find a wearable SHD item with STR to equip in CHEST if slot data is present
    let mut input = BuildInput {
        name: "test".into(),
        level: 60,
        classes: build_classes.clone(),
        race: snap.race_base_stats.keys().next().cloned(),
        ..Default::default()
    };
    let r = resolve_build(&snap, &input);
    assert!(r.wearable_item_count > 7000, "wearable count sane");
    assert!(r.buff_plan.buff_slots_used <= r.buff_plan.buff_slot_cap);
    assert!(r.stats.contains_key("STR"));
    // buffs contribute STR: Maniacal 68 + Focus of Spirit 67 + more lines
    assert!(r.stats["STR"].buffs >= 135.0, "STR buffs = {}", r.stats["STR"].buffs);
    // BLOCKER regression: 'HP (Hit Points)' / 'Attack (ATK)' labeled lines must land
    assert!(r.stats["HP"].buffs >= 400.0, "HP buffs = {}", r.stats["HP"].buffs);
    assert!(r.stats["ATK"].buffs > 0.0, "ATK buffs = {}", r.stats["ATK"].buffs);
    // coverage: every distinct DB statistic label maps or is explicitly skipped
    for line in &snap.buff_lines {
        if let Some(stat) = &line.statistic {
            let stripped = stat.split(" (").next().unwrap_or(stat).trim();
            assert!(
                eql_engine::stats::statistic_to_stat_key(stat).is_some()
                    || eql_engine::stats::STAT_SKIP_LIST.contains(&stripped),
                "unmapped buff statistic {stat:?} (line {}) — silently dropped",
                line.name
            );
        }
    }

    // ---- pet: NEC summon (Bone Walk 50150, base 9) at level 60 -> level 9 + tier 0
    input.pet_summon_spell_id = Some(50150);
    // give the pet Innoruuk's Curse (pageid 40, SHD, proc Soul Consumption req L50):
    // class-legal via the SHD intrinsic, stats active, proc INACTIVE at pet level 9
    input.pet_equipment.insert("PET_PRIMARY".into(), 40);
    let r2 = resolve_build(&snap, &input);
    let pet = r2.pet.expect("pet block");
    assert_eq!(pet.calculated_level, Some(9));
    assert_eq!(pet.intrinsic_classes, vec!["WAR".to_string(), "SHD".to_string()]);
    // pool = the PET's OWN classes (user-verified 2026-07-21), not the owner trio
    assert_eq!(pet.equip_class_pool, vec!["WAR".to_string(), "SHD".to_string()]);
    assert!(!pet.equip_class_pool.contains(&"MNK".to_string()), "owner classes stay out");
    assert_eq!(pet.buff_lines.len(), 5, "pet seed lines");
    // slot count = base 4 + the SUM of every class's bonus (VERIFIED_INGAME 2026-07-20:
    // L1 MAG/BST = 10 = 4+3+3, refuting max-wins). SHD/MNK/SHM = 4+0+0+1 = 5.
    assert_eq!(pet.slot_count, 5, "4 base + SHM 1 bonus");
    assert_eq!(pet.default_slot_count, 5);
    assert!(!pet.slot_count_overridden);
    assert_eq!(pet.slot_bonus_class.as_deref(), Some("SHM+1"));
    // the pet paperdoll always has all 23 wells; the budget limits ACTIVE ones
    assert_eq!(pet.gear.len(), 23);
    // 1 filled of 5 budget -> every empty well can still accept an item (green)
    assert!(pet.gear.iter().filter(|g| g.item_pageid.is_none()).all(|g| g.active),
            "empty wells stay active while budget remains");
    // the user's live confirmation: MAG/BST sums to 10
    let mut magbst = input.clone();
    magbst.classes = vec!["MAG".to_string(), "BST".to_string()];
    magbst.level = 1;
    let pet_mb = resolve_build(&snap, &magbst).pet.expect("pet block");
    assert_eq!(pet_mb.default_slot_count, 10, "L1 MAG/BST: 4 + 3 + 3 (bonuses SUM)");
    assert_eq!(pet_mb.slot_bonus_class.as_deref(), Some("MAG+3 BST+3"));
    // manual override wins over the derived count and clamps to PET_SLOT_MAX
    input.pet_slot_override = Some(99);
    let r_ov = resolve_build(&snap, &input);
    let pet_ov = r_ov.pet.expect("pet block");
    assert_eq!(pet_ov.slot_count, eql_data::PET_SLOT_MAX);
    assert!(pet_ov.slot_count_overridden);
    assert_eq!(pet_ov.default_slot_count, 5, "default still reported alongside the override");
    input.pet_slot_override = None;
    // official rule (7/7 notes + research workbook): only ACTUALLY GAINED levels grant
    // stats — tier ranks eaten by the player-1 cap grant nothing
    let mut low = input.clone();
    low.level = 10; // Bone Walk base 9: cap = 10-1 = 9 -> zero headroom
    low.spell_tiers.insert(50150, 5);
    let pet_low = resolve_build(&snap, &low).pet.expect("pet block");
    assert_eq!(pet_low.calculated_level, Some(9), "hard-capped at player-1");
    assert_eq!(pet_low.levels_gained, Some(0));
    assert!(pet_low.tier_capped);
    assert_eq!(pet_low.skill_point_bonus, 0, "capped ranks grant no skill points");
    assert_eq!(pet_low.pet_hp_scaled, pet_low.summon.pet_hp, "no HP from capped ranks");
    // partial cap: level 12 -> cap 11 -> only 2 of the 5 ranks land
    low.level = 12;
    let pet_mid = resolve_build(&snap, &low).pet.expect("pet block");
    assert_eq!(pet_mid.calculated_level, Some(11));
    assert_eq!(pet_mid.levels_gained, Some(2));
    assert!(pet_mid.tier_capped);
    assert_eq!(pet_mid.skill_point_bonus, 10);
    if let (Some(base_hp), Some(scaled)) = (pet_mid.summon.pet_hp, pet_mid.pet_hp_scaled) {
        assert_eq!(scaled, ((base_hp as f64) * 1.12).floor() as i64, "+6% x 2 gained");
    }

    // a zero-bonus class (SHD: 0) must not be CREDITED as the bonus source: a pure-SHD
    // build gets 4 slots with slot_bonus_class None (no false "+ SHD bonus" label)
    let mut shd_only = input.clone();
    shd_only.classes = vec!["SHD".to_string()];
    let pet_shd = resolve_build(&snap, &shd_only).pet.expect("pet block");
    assert_eq!(pet_shd.default_slot_count, 4, "base 4 + no bonus");
    assert_eq!(pet_shd.slot_bonus_class, None, "zero bonus is not a bonus source");
    let g1 = pet.gear.iter().find(|g| g.slot == "PET_PRIMARY").expect("primary well");
    assert_eq!(g1.badge, "PROC_INACTIVE", "{:?}", g1.reason);
    assert!(g1.active, "within budget");
    assert!(g1.reason.as_deref().unwrap().contains("level 50"));
    assert!(pet.gear_totals.contains_key("AC") || !pet.gear_totals.is_empty(),
            "stats still count while the proc is inactive (plan §15)");

    // ---- over-cap: filled wells consume the budget in ROW ORDER; extras go red.
    // Override the budget down to 1 and fill HEAD (row 1) + PRIMARY (row 4): HEAD is
    // the one active slot, the weapon goes OVER_CAP and stops contributing.
    let mut over = input.clone();
    over.pet_slot_override = Some(1);
    over.pet_equipment.insert("PET_HEAD".into(), 40);
    let pet_over = resolve_build(&snap, &over).pet.expect("pet block");
    assert_eq!(pet_over.slot_count, 1);
    let head = pet_over.gear.iter().find(|g| g.slot == "PET_HEAD").unwrap();
    let prim = pet_over.gear.iter().find(|g| g.slot == "PET_PRIMARY").unwrap();
    assert!(head.active, "first filled well in row order stays active");
    assert_eq!(prim.badge, "OVER_CAP", "{:?}", prim.reason);
    assert!(!prim.active);
    assert!(pet_over.weapon_config.is_empty(),
            "an over-cap weapon must not reach the hand rule");
    assert!(pet_over.gear.iter().filter(|g| g.item_pageid.is_none()).all(|g| !g.active),
            "budget exhausted -> every empty well goes red");

    // ---- ANY slot accepts a class-legal item regardless of its wear slot
    let potion = snap.items_by_id.values()
        .find(|i| (i.classes.iter().any(|c| c == "ALL") || i.classes.iter().any(|c| c == "SHM"))
              && i.stats.values().any(|v| *v > 0)
              && eql_data::era_allowed(i.era.as_deref(), &[]));
    if let Some(p) = potion {
        let mut any_input = input.clone();
        any_input.equipment.insert("ANY1".into(), p.pageid);
        let ra = resolve_build(&snap, &any_input);
        assert!(!ra.equipment_warnings.iter()
            .any(|w| w.item == p.name && w.status == "SAVED_INACTIVE"),
            "ANY slot wrongly rejected {}: {:?}", p.name, ra.equipment_warnings);
    }

    // ---- strict buffs: a self-cast buff needs its spell scribed
    let mut strict_input = input.clone();
    strict_input.strict_buffs = true;
    let rs = resolve_build(&snap, &strict_input);
    // with an empty spellbook, no line resolves to SELF_CAST
    let self_cast_strict = rs.buff_plan.lines.iter()
        .filter(|l| l.chosen.as_ref().map(|c| c.status) == Some(MemberStatus::SelfCast))
        .count();
    assert_eq!(self_cast_strict, 0, "strict mode with empty spellbook must scribe nothing");
    // scribe Maniacal Strength (find its pageid) -> its line self-casts again
    if let Some((&msid, _)) = snap.spell_names.iter().find(|(_, n)| *n == "Maniacal Strength") {
        strict_input.spellbook.insert(0, msid);
        let rs2 = resolve_build(&snap, &strict_input);
        assert!(rs2.buff_plan.lines.iter().any(|l|
            l.chosen.as_ref().map(|c| c.name.as_str()) == Some("Maniacal Strength")
            && l.chosen.as_ref().map(|c| c.status) == Some(MemberStatus::SelfCast)),
            "scribed spell should self-cast under strict mode");
    }

    // ---- disabled buff drops from the plan
    let mut dis_input = input.clone();
    dis_input.disabled_buffs = vec!["Maniacal Strength".into()];
    let rd = resolve_build(&snap, &dis_input);
    assert!(!rd.buff_plan.active.iter().any(|a| a.name == "Maniacal Strength"),
            "disabled buff must not be active");

    // ---- summon tier raises pet level + scales HP (Bone Walk base 9)
    let mut pt_input = input.clone();
    pt_input.pet_summon_spell_id = Some(50150);
    pt_input.spell_tiers.insert(50150, 3); // +3 tier
    let rp = resolve_build(&snap, &pt_input);
    let pb = rp.pet.expect("pet");
    assert_eq!(pb.effective_tier, 3);
    assert_eq!(pb.calculated_level, Some(12)); // 9 + 3, well under level-1-below-60
    assert!(pb.pet_hp_scaled.unwrap() > pb.summon.pet_hp.unwrap()); // +6%/tier

    // ---- AA planner: costs are cumulative, and Mnemonic Retention drives the gems
    assert!(snap.aas.len() >= 130, "AA table loaded ({} rows)", snap.aas.len());
    let mnem = snap.aas.iter().find(|a| a.name == "Mnemonic Retention").expect("Mnemonic AA");
    assert_eq!(mnem.max_rank, 6);
    assert_eq!(mnem.costs, vec![Some(1), Some(1), Some(2), Some(2), Some(3), Some(3)]);
    let mut aa_input = input.clone();
    aa_input.aa_points_available = 20;
    aa_input.aa_ranks.insert(mnem.id, 3); // 1+1+2 = 4 points, +3 gems
    let ra = resolve_build(&snap, &aa_input);
    assert_eq!(ra.aa_plan.points_spent, 4, "cumulative rank cost");
    assert_eq!(ra.aa_plan.points_available, 20);
    assert_eq!(ra.spell_gem_count, 11, "8 base + 3 ranks");
    assert!(!ra.aa_plan.cost_is_lower_bound);
    // a CLASS aa from a class we don't have is kept but flagged
    if let Some(foreign) = snap.aas.iter().find(|a| {
        a.category == "CLASS"
            && a.class_abbr.as_deref().is_some_and(|c| !["SHD", "MNK", "SHM"].contains(&c))
    }) {
        aa_input.aa_ranks.insert(foreign.id, 1);
        let rb = resolve_build(&snap, &aa_input);
        assert!(rb.aa_plan.class_locked.contains(&foreign.name),
                "AA from another class must be flagged: {:?}", rb.aa_plan.class_locked);
    }

    // ---- expansion toggle (plan §18 semantics: saved-but-inactive, not deleted)
    let velious_item = snap.items_by_id.values()
        .find(|i| i.era.as_deref() == Some("Velious")
              && i.classes.iter().any(|c| c == "ALL" || c == "SHD")
              && !i.slots.is_empty() && i.required_level.is_none())
        .expect("a Velious SHD-wearable item");
    let mut era_input = input.clone();
    era_input.equipment.insert(velious_item.slots[0].clone(), velious_item.pageid);
    era_input.enabled_eras = vec!["Classic".into(), "Kunark".into()];
    let re = resolve_build(&snap, &era_input);
    let w = re.equipment_warnings.iter()
        .find(|w| w.item == velious_item.name)
        .expect("out-of-era warning");
    assert_eq!(w.status, "SAVED_INACTIVE");
    assert!(w.reason.contains("out of enabled expansions (Velious)"), "{}", w.reason);
    // same item with all eras enabled -> no era warning
    era_input.enabled_eras = vec![];
    let re2 = resolve_build(&snap, &era_input);
    assert!(!re2.equipment_warnings.iter()
        .any(|w| w.item == velious_item.name && w.reason.contains("out of enabled")));

    // ---- chooser: deterministic, produces legal equipment; respects the era toggle
    let classic_only = eql_engine::choose_for_me(
        &snap, 7, 60, vec!["SHD".into(), "MNK".into(), "SHM".into()],
        vec!["Classic".into()]);
    for pid in classic_only.equipment.values() {
        let it = &snap.items_by_id[pid];
        assert!(it.era.is_none() || it.era.as_deref() == Some("Classic"),
                "chooser picked out-of-era {} ({:?})", it.name, it.era);
    }
    let a = eql_engine::choose_for_me(&snap, 7, 60,
        vec!["SHD".into(), "MNK".into(), "SHM".into()], vec![]);
    let b = eql_engine::choose_for_me(&snap, 7, 60,
        vec!["SHD".into(), "MNK".into(), "SHM".into()], vec![]);
    assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
    let rc = resolve_build(&snap, &a);
    let inactive = rc.equipment_warnings.iter().filter(|w| w.status == "SAVED_INACTIVE").count();
    assert_eq!(inactive, 0, "chooser must produce fully-active equipment: {:?}",
               rc.equipment_warnings);
    assert!(a.equipment.len() >= 15, "chooser filled {} of 21 slots", a.equipment.len());
    assert!(rc.stats["STR"].equipment > 0, "chosen gear contributes stats");
    assert!(rc.stats["STR"].base > 0, "race+class base stats present");
}

#[test]
fn builds_db_round_trip() {
    point_at_repo_db();
    let snap = db::snapshot();
    let mut input = eql_engine::choose_for_me(&snap, 99, 50,
        vec!["NEC".into(), "ENC".into(), "MAG".into()], vec![]);
    // tier persistence: player slot, pet slot, and a spell tier all round-trip
    let first_slot = input.equipment.keys().next().cloned().expect("has equipment");
    input.equipment_tiers.insert(first_slot.clone(), 7);
    if let Some(sid) = input.pet_summon_spell_id {
        input.spell_tiers.insert(sid, 4);
    }
    // TWO spells missing from this mirror (a shared build from a newer wiki): their
    // fallback canonical names must not collide on the (build_id, name) PK
    input.spell_tiers.insert(999_000_111, 3);
    input.spell_tiers.insert(999_000_222, 5);
    // augments round-trip too (socket map on a player slot)
    input.augments
        .entry(first_slot)
        .or_default()
        .insert("PROC".into(), 42_663); // Hierophant`s Crook
    input.enabled_eras = vec!["Classic".into(), "Kunark".into()];
    // power-planner externals round-trip too
    input.external_buffs = vec![50100, 50200];
    let id = eql_builder_lib::builds::save_build(&input).unwrap();
    let loaded = eql_builder_lib::builds::load_build(id).unwrap();
    assert_eq!(loaded.name, input.name);
    assert_eq!(loaded.classes, input.classes);
    assert_eq!(loaded.level, input.level);
    assert_eq!(loaded.equipment, input.equipment);
    assert_eq!(loaded.equipment_tiers, input.equipment_tiers);
    assert_eq!(loaded.spell_tiers, input.spell_tiers,
               "unknown-pageid spell tiers must not collide and vanish");
    assert_eq!(loaded.augments, input.augments);
    assert_eq!(loaded.external_buffs, input.external_buffs);
    assert_eq!(loaded.enabled_eras, input.enabled_eras);
    assert_eq!(loaded.pet_summon_spell_id, input.pet_summon_spell_id);
    let list = eql_builder_lib::builds::list_builds().unwrap();
    assert!(list.iter().any(|b| b.id == id));
    eql_builder_lib::builds::delete_build(id).unwrap();
}

/// A fresh install now starts with NO classes selected (state.svelte.ts newBuild).
/// Resolving that empty default must not panic and must produce a sane empty block.
#[test]
fn empty_default_build_resolves_cleanly() {
    point_at_repo_db();
    let snap = db::snapshot();
    let b = BuildInput { name: "New build".into(), level: 50, ..Default::default() };
    assert!(b.classes.is_empty());
    let r = resolve_build(&snap, &b); // must not panic
    // no classes -> no class-specific spells/buffs and base HP/mana 0 (only ALL-class
    // gear stays "wearable", which is correct — anyone can wear it)
    assert!(r.buff_plan.active.is_empty(), "no classes -> no buffs");
    assert_eq!(r.stats["HP"].capped_total, 0.0, "no classes -> base HP 0");
    assert_eq!(r.stats["MANA"].capped_total, 0.0, "no classes -> base mana 0");
    assert!(r.pet.is_none());
}
