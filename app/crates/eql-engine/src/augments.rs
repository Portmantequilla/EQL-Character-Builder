//! Exaltation augments (user-reported system, verified against a live Keg Mallet +5
//! item window 2026-07-15): every effect-bearing item — EXCEPT regen effects, so far —
//! can have its effect EXTRACTED once the item reaches +4, becoming an augment named
//! "<item> (Exaltation)" that sockets into other gear. The effect transfers; stats
//! never move. Sockets per item: Ornamentation (cosmetic) + Focus/Click/Worn/Proc
//! Exaltation, matching the four item_effect activation types.
use crate::Snapshot;
use eql_data::{AugmentGrant, AugmentInfo, BuildInput, Item};
use std::collections::BTreeMap;

/// Extraction gate: the source item must be at this upgrade tier before its effect can
/// be removed (user-verified in game, 2026-07-15). Fallback for `extract_min_tier`.
pub const DEFAULT_EXTRACT_MIN_TIER: u32 = 4;

/// The extraction gate as currently configured (formula_table key
/// `exaltation_extract_min_tier`, editable in Settings) — surfaced to the UI so the
/// popup's "+N to extract" hint tracks edits instead of hardcoding 4.
pub fn extract_min_tier(snapshot: &Snapshot) -> u32 {
    snapshot
        .formulas
        .get("exaltation_extract_min_tier")
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_EXTRACT_MIN_TIER)
}

fn is_regen(effect_name: &str) -> bool {
    effect_name.to_ascii_lowercase().contains("regen")
}

/// The hand slots. A source restricted to these restricts its augment's hosts too
/// (user: augments "have rules such as Class, Primary, Secondary etc").
fn hand_slots(item: &Item) -> Vec<String> {
    item.slots
        .iter()
        .filter(|s| {
            let u = s.to_uppercase();
            u == "PRIMARY" || u == "SECONDARY" || u == "RANGE"
        })
        .map(|s| s.to_uppercase())
        .collect()
}

/// Is the source item ONLY wearable in hand slots (weapon-like)? Such an augment's
/// host must share one of those hands; armor-sourced augments have no slot rule.
fn hand_only(item: &Item) -> bool {
    let hands = hand_slots(item);
    !hands.is_empty()
        && item
            .slots
            .iter()
            .all(|s| matches!(s.to_uppercase().as_str(), "PRIMARY" | "SECONDARY" | "RANGE"))
}

/// Every augment the item data can source: one entry per (item, effect) with the
/// effect kind as the socket type. Regen effects are excluded (not extractable — user
/// report; revisit if the game changes it).
pub fn augment_catalog(snapshot: &Snapshot) -> Vec<AugmentInfo> {
    let mut out = Vec::new();
    for (pid, effects) in &snapshot.item_effects {
        let Some(item) = snapshot.items_by_id.get(pid) else { continue };
        for e in effects {
            let socket = e.activation_type.to_uppercase();
            if !matches!(socket.as_str(), "FOCUS" | "CLICK" | "WORN" | "PROC") {
                continue;
            }
            if is_regen(&e.effect_name) {
                continue;
            }
            out.push(AugmentInfo {
                source_pageid: *pid,
                name: format!("{} (Exaltation)", item.name),
                socket,
                effect_name: e.effect_name.clone(),
                required_level: e.required_level,
                classes: item.classes.clone(),
                races: item.races.clone(),
                slots: item.slots.iter().map(|s| s.to_uppercase()).collect(),
                flags: item.flags.clone(),
                spell_id: e.spell_id,
                era: item.era.clone(),
                icon_id: item.icon_id,
            });
        }
    }
    out.sort_by(|a, b| a.socket.cmp(&b.socket).then(a.name.cmp(&b.name)));
    out
}

/// Validate + resolve every socketed augment in the build (player paperdoll AND pet
/// "PET_<SLOT>" wells). Rule breaks WARN — the planner never hard-blocks what the user says their
/// character has — but a socket whose SOURCE can't grant that effect kind at all
/// (unknown item / wrong socket / regen) still resolves with a warning and no effect
/// name is invented.
pub fn resolve_augments(
    snapshot: &Snapshot,
    build: &BuildInput,
) -> BTreeMap<String, Vec<AugmentGrant>> {
    let mut out: BTreeMap<String, Vec<AugmentGrant>> = BTreeMap::new();
    let player_classes: Vec<String> = build.classes.iter().map(|c| c.to_uppercase()).collect();
    // pet gear follows the PET's classes (user-verified 2026-07-21): a WAR/BST pet
    // takes WAR-restricted Exaltations the owner trio couldn't use
    let pet_classes = crate::pet::pet_class_pool(snapshot, build);
    for (slot, sockets) in &build.augments {
        let (classes, who) = if slot.starts_with("PET_") {
            (&pet_classes, "pet classes are")
        } else {
            (&player_classes, "build is")
        };
        let host_pid = build
            .equipment
            .get(slot)
            .or_else(|| build.pet_equipment.get(slot));
        let host = host_pid.and_then(|p| snapshot.items_by_id.get(p));
        for (socket, source_pid) in sockets {
            let socket = socket.to_uppercase();
            let mut warnings = Vec::new();
            let source = snapshot.items_by_id.get(source_pid);
            let (name, effect_name, required_level) = match source {
                None => {
                    warnings.push("source item missing from wiki data".to_string());
                    (format!("item #{source_pid} (Exaltation)"), String::new(), None)
                }
                Some(src) => {
                    // the source's effect OF THIS KIND is what transfers
                    let eff = snapshot
                        .item_effects
                        .get(source_pid)
                        .into_iter()
                        .flatten()
                        .find(|e| e.activation_type.eq_ignore_ascii_case(&socket));
                    let (ename, rlvl) = match eff {
                        Some(e) if is_regen(&e.effect_name) => {
                            warnings.push(format!(
                                "{} is a regen effect — not extractable (so far)",
                                e.effect_name
                            ));
                            (e.effect_name.clone(), e.required_level)
                        }
                        Some(e) => (e.effect_name.clone(), e.required_level),
                        None => {
                            warnings.push(format!(
                                "{} has no {} effect in the wiki data",
                                src.name,
                                socket.to_lowercase()
                            ));
                            (String::new(), None)
                        }
                    };
                    // class rule: the source's class restriction follows the augment;
                    // the wearer is the PET for pet slots, the character otherwise
                    let class_ok = src.classes.is_empty()
                        || src.classes.iter().any(|c| c == "ALL")
                        || src.classes.iter().any(|c| classes.iter().any(|b| b == c));
                    if !class_ok {
                        warnings.push(format!(
                            "class-restricted: {} ({who} {})",
                            src.classes.join("/"),
                            classes.join("/")
                        ));
                    }
                    // hand rule: a weapon-sourced augment needs a host sharing a hand slot
                    if hand_only(src) {
                        let src_hands = hand_slots(src);
                        let host_ok = host.is_some_and(|h| {
                            h.slots.iter().any(|s| src_hands.contains(&s.to_uppercase()))
                        });
                        if !host_ok {
                            warnings.push(format!(
                                "slot-restricted: source fits {} — host doesn't share that slot",
                                src_hands.join("/")
                            ));
                        }
                    }
                    // era rule: a source outside the enabled expansions can't exist yet
                    // (advisory — imported/legacy sockets still resolve and display)
                    if !eql_data::era_allowed(src.era.as_deref(), &build.enabled_eras) {
                        warnings.push(format!(
                            "out of enabled expansions ({})",
                            src.era.as_deref().unwrap_or("?")
                        ));
                    }
                    (format!("{} (Exaltation)", src.name), ename, rlvl)
                }
            };
            if host_pid.is_none() {
                warnings.push("no item equipped in this slot".to_string());
            }
            out.entry(slot.clone()).or_default().push(AugmentGrant {
                slot: slot.clone(),
                socket,
                source_pageid: *source_pid,
                name,
                effect_name,
                required_level,
                warnings,
            });
        }
    }
    // stable socket order within a slot (the item window's display order)
    let order = |s: &str| eql_data::AUGMENT_SOCKETS.iter().position(|x| *x == s).unwrap_or(9);
    for grants in out.values_mut() {
        grants.sort_by_key(|g| order(&g.socket));
    }
    out
}

/// Everything the character's WORN gear (player paperdoll only — pet effects live on
/// the Pet tab) grants: the items' own effects plus every socketed augment's effect.
/// Display-only: nothing here is folded into stat totals yet (no formulas collected).
pub fn effect_overview(
    snapshot: &Snapshot,
    build: &BuildInput,
    grants: &BTreeMap<String, Vec<eql_data::AugmentGrant>>,
) -> Vec<eql_data::ItemEffectOverview> {
    let mut out = Vec::new();
    let slot_order = |s: &str| {
        eql_data::PAPERDOLL_SLOTS.iter().position(|x| *x == s).unwrap_or(99)
    };
    let display_name = |slot: &str, item: &Item| {
        let tier = build.equipment_tiers.get(slot).copied().unwrap_or(0);
        if tier > 0 { format!("{} +{tier}", item.name) } else { item.name.clone() }
    };
    // the items' own effects
    for (slot, pid) in &build.equipment {
        let Some(item) = snapshot.items_by_id.get(pid) else { continue };
        for e in snapshot.item_effects.get(pid).into_iter().flatten() {
            let kind = e.activation_type.to_uppercase();
            out.push(eql_data::ItemEffectOverview {
                label: eql_data::augment_effect_label(&kind).to_string(),
                kind,
                effect_name: e.effect_name.clone(),
                source_slot: slot.clone(),
                source_item: display_name(slot, item),
                via_augment: None,
                required_level: e.required_level,
                spell_id: e.spell_id,
                level_gated: e.required_level.is_some_and(|r| r > build.level as i64),
                warnings: Vec::new(),
            });
        }
    }
    // effects arriving through socketed Exaltations (player slots only)
    for (slot, gs) in grants {
        if slot.starts_with("PET_") {
            continue;
        }
        let host = build
            .equipment
            .get(slot)
            .and_then(|p| snapshot.items_by_id.get(p));
        for g in gs {
            if g.effect_name.is_empty() {
                continue; // unresolvable socket — already warned in the grant itself
            }
            // the source's matching effect row carries the spell link
            let spell_id = snapshot
                .item_effects
                .get(&g.source_pageid)
                .into_iter()
                .flatten()
                .find(|e| e.activation_type.eq_ignore_ascii_case(&g.socket))
                .and_then(|e| e.spell_id);
            out.push(eql_data::ItemEffectOverview {
                label: eql_data::augment_effect_label(&g.socket).to_string(),
                kind: g.socket.clone(),
                effect_name: g.effect_name.clone(),
                source_slot: slot.clone(),
                source_item: host
                    .map(|h| display_name(slot, h))
                    .unwrap_or_else(|| slot.clone()),
                via_augment: Some(g.name.clone()),
                required_level: g.required_level,
                spell_id,
                level_gated: g.required_level.is_some_and(|r| r > build.level as i64),
                warnings: g.warnings.clone(),
            });
        }
    }
    out.sort_by(|a, b| {
        slot_order(&a.source_slot)
            .cmp(&slot_order(&b.source_slot))
            .then_with(|| a.via_augment.is_some().cmp(&b.via_augment.is_some()))
            .then_with(|| a.effect_name.cmp(&b.effect_name))
    });
    out
}

/// Flat stat bonuses from WORN effects the character actually wears: the items' own
/// WORN effects (ACTIVE player slots only) plus WORN Exaltation grants. An effect whose
/// name is already an ACTIVE buff in the plan is skipped — a worn buff the user enabled
/// via "Add Other Buff" must never count twice. Level-gated effects grant nothing.
/// Amounts come from Snapshot.spell_stat_effects (flat spell_effect rows; percent rows
/// like haste are excluded at load — item haste already counts via items.haste_pct).
pub fn worn_stat_totals(
    snapshot: &Snapshot,
    build: &BuildInput,
    grants: &BTreeMap<String, Vec<eql_data::AugmentGrant>>,
    active_items: &[i64],
    plan: &eql_data::BuffPlan,
) -> BTreeMap<String, f64> {
    let active_buffs: std::collections::BTreeSet<&str> =
        plan.active.iter().map(|a| a.name.as_str()).collect();
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    fn add_spell(out: &mut BTreeMap<String, f64>, snapshot: &Snapshot, sid: i64) {
        for (stat, amt) in snapshot.spell_stat_effects.get(&sid).into_iter().flatten() {
            *out.entry(stat.clone()).or_default() += amt;
        }
    }
    // the items' own WORN effects
    for pid in build.equipment.values() {
        if !active_items.contains(pid) {
            continue; // saved-but-inactive gear grants nothing
        }
        for e in snapshot.item_effects.get(pid).into_iter().flatten() {
            if e.activation_type != "WORN" {
                continue;
            }
            if e.required_level.is_some_and(|r| r > build.level as i64) {
                continue;
            }
            if active_buffs.contains(e.effect_name.as_str()) {
                continue;
            }
            if let Some(sid) = e.spell_id {
                add_spell(&mut out, snapshot, sid);
            }
        }
    }
    // WORN Exaltation grants socketed into player gear
    for (slot, gs) in grants {
        if slot.starts_with("PET_") {
            continue; // pet effects live on the Pet tab, not the character sheet
        }
        let Some(pid) = build.equipment.get(slot) else { continue };
        if !active_items.contains(pid) {
            continue;
        }
        for g in gs {
            if g.socket != "WORN" || g.effect_name.is_empty() {
                continue;
            }
            if g.required_level.is_some_and(|r| r > build.level as i64) {
                continue;
            }
            if active_buffs.contains(g.effect_name.as_str()) {
                continue;
            }
            let sid = snapshot
                .item_effects
                .get(&g.source_pageid)
                .into_iter()
                .flatten()
                .find(|e| e.activation_type == "WORN")
                .and_then(|e| e.spell_id);
            if let Some(sid) = sid {
                add_spell(&mut out, snapshot, sid);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use eql_data::ItemEffect;

    fn item(pid: i64, name: &str, slots: &[&str], classes: &[&str]) -> Item {
        Item {
            pageid: pid,
            name: name.into(),
            slots: slots.iter().map(|s| s.to_string()).collect(),
            classes: classes.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }
    fn effect(kind: &str, name: &str, req: Option<i64>) -> ItemEffect {
        ItemEffect {
            effect_name: name.into(),
            activation_type: kind.into(),
            required_level: req,
            spell_id: None,
        }
    }

    fn snapshot() -> Snapshot {
        let mut s = Snapshot::default();
        for it in [
            item(1, "Hierophant's Crook", &["PRIMARY"], &["DRU"]),   // PROC Earthquake 30
            item(2, "Keg Mallet", &["PRIMARY"], &["DRU"]),           // host
            item(3, "Wicked Sallet", &["HEAD"], &["ALL"]),           // FOCUS
            item(4, "Regen Ring", &["FINGER"], &["ALL"]),            // WORN regen (excluded)
            item(5, "Cleric Mace", &["PRIMARY"], &["CLR"]),          // class-restricted PROC
        ] {
            s.items_by_id.insert(it.pageid, it);
        }
        s.item_effects.insert(1, vec![effect("PROC", "Earthquake", Some(30))]);
        s.item_effects.insert(3, vec![effect("FOCUS", "Mana Preservation I", None)]);
        s.item_effects.insert(4, vec![effect("WORN", "Regeneration", None)]);
        s.item_effects.insert(5, vec![effect("PROC", "Holy Smite", Some(10))]);
        s
    }

    fn build() -> BuildInput {
        let mut b = BuildInput {
            classes: vec!["DRU".into()],
            level: 50,
            ..Default::default()
        };
        b.equipment.insert("PRIMARY".into(), 2); // Keg Mallet in hand
        b
    }

    #[test]
    fn catalog_excludes_regen_and_types_sockets() {
        let cat = augment_catalog(&snapshot());
        assert!(cat.iter().any(|a| a.name == "Hierophant's Crook (Exaltation)" && a.socket == "PROC"));
        assert!(cat.iter().any(|a| a.socket == "FOCUS"));
        assert!(!cat.iter().any(|a| a.effect_name.contains("Regen")), "regen not extractable");
    }

    #[test]
    fn keg_mallet_screenshot_scenario() {
        // the exact live case: Hierophant's Crook proc socketed into the Keg Mallet
        let mut b = build();
        b.augments
            .entry("PRIMARY".into())
            .or_default()
            .insert("PROC".into(), 1);
        let grants = resolve_augments(&snapshot(), &b);
        let g = &grants["PRIMARY"][0];
        assert_eq!(g.name, "Hierophant's Crook (Exaltation)");
        assert_eq!(g.effect_name, "Earthquake");
        assert_eq!(g.required_level, Some(30));
        assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    }

    #[test]
    fn class_and_slot_rules_warn_not_block() {
        let mut b = build();
        // CLR-only proc into a DRU build's weapon: class warning
        b.augments.entry("PRIMARY".into()).or_default().insert("PROC".into(), 5);
        // weapon-sourced proc into HEAD armor: hand-slot warning
        b.equipment.insert("HEAD".into(), 3);
        b.augments.entry("HEAD".into()).or_default().insert("PROC".into(), 1);
        let grants = resolve_augments(&snapshot(), &b);
        assert!(grants["PRIMARY"][0].warnings.iter().any(|w| w.contains("class-restricted")));
        assert!(grants["HEAD"][0].warnings.iter().any(|w| w.contains("slot-restricted")));
        // both still resolve (advisory, not blocking)
        assert_eq!(grants["PRIMARY"][0].effect_name, "Holy Smite");
        assert_eq!(grants["HEAD"][0].effect_name, "Earthquake");
    }

    #[test]
    fn pet_slot_class_rule_uses_pet_classes() {
        // the user's live case (2026-07-21): a WAR/BST pet (Spirit of Kashek) wearing a
        // MNK/BST weapon takes a WAR-restricted Exaltation — the owner trio (no WAR)
        // must NOT trigger the class warning; the same augment on the PLAYER's gear must
        let mut snap = snapshot();
        snap.items_by_id.insert(6, item(6, "Blade of Tactics", &["PRIMARY"], &["WAR"]));
        snap.item_effects.insert(6, vec![effect("WORN", "Aura of Tactics", None)]);
        snap.items_by_id.insert(7, item(7, "Flaming Fist", &["PRIMARY"], &["MNK", "BST"]));
        snap.pet_summons.insert(
            900,
            eql_data::PetSummonInfo {
                spell_id: 900,
                name: "Spirit of Kashek".into(),
                base_pet_level: Some(10),
                pet_classes: Some("WAR/BST".into()),
                pet_hp: None,
                pet_max_hit: None,
                class_levels: BTreeMap::from([("BST".into(), 10u32)]),
                estimate_confidence: None,
            },
        );
        let mut b = build(); // DRU character
        b.pet_summon_spell_id = Some(900);
        b.pet_equipment.insert("PET_PRIMARY".into(), 7);
        b.augments.entry("PET_PRIMARY".into()).or_default().insert("WORN".into(), 6);
        // same WAR-only augment socketed into the DRU's own weapon
        b.augments.entry("PRIMARY".into()).or_default().insert("WORN".into(), 6);
        let grants = resolve_augments(&snap, &b);
        let pet_g = &grants["PET_PRIMARY"][0];
        assert!(
            !pet_g.warnings.iter().any(|w| w.contains("class-restricted")),
            "WAR augment is legal on a WAR/BST pet: {:?}",
            pet_g.warnings
        );
        assert_eq!(pet_g.effect_name, "Aura of Tactics");
        let player_g = &grants["PRIMARY"][0];
        assert!(
            player_g.warnings.iter().any(|w| w.contains("class-restricted")),
            "the DRU character still warns: {:?}",
            player_g.warnings
        );
    }

    #[test]
    fn wrong_socket_and_empty_slot_warn() {
        let mut b = build();
        // the Sallet has FOCUS only — socketing it as WORN can't grant anything
        b.augments.entry("PRIMARY".into()).or_default().insert("WORN".into(), 3);
        // augment on a slot with nothing equipped
        b.augments.entry("CHEST".into()).or_default().insert("FOCUS".into(), 3);
        let grants = resolve_augments(&snapshot(), &b);
        let worn = &grants["PRIMARY"][0];
        assert!(worn.effect_name.is_empty(), "no effect invented");
        assert!(worn.warnings.iter().any(|w| w.contains("no worn effect")));
        assert!(grants["CHEST"][0].warnings.iter().any(|w| w.contains("no item equipped")));
    }

    #[test]
    fn overview_lists_own_and_augment_effects() {
        // Keg Mallet +5 hosting the Crook's proc; Sallet worn with its OWN focus effect
        let mut b = build();
        b.equipment_tiers.insert("PRIMARY".into(), 5);
        b.equipment.insert("HEAD".into(), 3);
        b.augments.entry("PRIMARY".into()).or_default().insert("PROC".into(), 1);
        let snap = snapshot();
        let grants = resolve_augments(&snap, &b);
        let ov = effect_overview(&snap, &b, &grants);
        // the Sallet's own FOCUS effect
        let own = ov.iter().find(|e| e.via_augment.is_none()).expect("own effect");
        assert_eq!(own.effect_name, "Mana Preservation I");
        assert_eq!(own.label, "Focus Effect");
        // the augment-granted proc, attributed to the tiered host + the Exaltation
        let aug = ov.iter().find(|e| e.via_augment.is_some()).expect("augment effect");
        assert_eq!(aug.effect_name, "Earthquake");
        assert_eq!(aug.label, "Combat Effect");
        assert_eq!(aug.source_item, "Keg Mallet +5");
        assert_eq!(aug.via_augment.as_deref(), Some("Hierophant's Crook (Exaltation)"));
        // Earthquake req 30 vs level 50: not gated
        assert!(!aug.level_gated);
        // pet slots stay off the character overview
        b.pet_equipment.insert("PET_PRIMARY".into(), 5);
        b.augments.entry("PET_PRIMARY".into()).or_default().insert("PROC".into(), 1);
        let grants = resolve_augments(&snap, &b);
        let ov = effect_overview(&snap, &b, &grants);
        assert!(ov.iter().all(|e| !e.source_slot.starts_with("PET_")));
    }

    #[test]
    fn worn_effects_fold_into_stats() {
        let mut snap = snapshot();
        // item 8: worn "Aura of Might" (spell 70: +10 ATK, +5 SV MAGIC), req level 20
        snap.items_by_id.insert(8, item(8, "Mighty Cloak", &["BACK"], &["ALL"]));
        snap.item_effects.insert(8, vec![ItemEffect {
            effect_name: "Aura of Might".into(), activation_type: "WORN".into(),
            required_level: Some(20), spell_id: Some(70),
        }]);
        snap.spell_stat_effects.insert(70, vec![("ATK".into(), 10.0), ("SV MAGIC".into(), 5.0)]);
        // source 6: WORN Exaltation "Aura of Tactics" (spell 71: +7 AC)
        snap.items_by_id.insert(6, item(6, "Blade of Tactics", &["PRIMARY"], &["ALL"]));
        snap.item_effects.insert(6, vec![ItemEffect {
            effect_name: "Aura of Tactics".into(), activation_type: "WORN".into(),
            required_level: None, spell_id: Some(71),
        }]);
        snap.spell_stat_effects.insert(71, vec![("AC".into(), 7.0)]);
        let mut b = build(); // level 50, Keg Mallet (pid 2) in PRIMARY
        b.equipment.insert("BACK".into(), 8);
        b.augments.entry("PRIMARY".into()).or_default().insert("WORN".into(), 6);
        let grants = resolve_augments(&snap, &b);
        let plan = eql_data::BuffPlan::default();
        let fx = worn_stat_totals(&snap, &b, &grants, &[2, 8], &plan);
        assert_eq!(fx.get("ATK"), Some(&10.0), "own worn effect folds");
        assert_eq!(fx.get("SV MAGIC"), Some(&5.0));
        assert_eq!(fx.get("AC"), Some(&7.0), "WORN Exaltation grant folds");
        // level gate: below the requirement the effect grants nothing
        b.level = 10;
        let fx_low = worn_stat_totals(&snap, &b, &grants, &[2, 8], &plan);
        assert_eq!(fx_low.get("ATK"), None, "level-gated worn effect grants nothing");
        assert_eq!(fx_low.get("AC"), Some(&7.0), "ungated grant still folds");
        // dedup: the effect active as a BUFF must not count twice
        b.level = 50;
        let mut plan_active = eql_data::BuffPlan::default();
        plan_active.active.push(eql_data::ActiveBuff {
            name: "Aura of Might".into(),
            lines: Vec::new(),
            total_value: 10.0,
            status: eql_data::MemberStatus::SelfCast,
            why: String::new(),
            source_tag: "OTHER".into(),
            duration: None,
        });
        let fx_dedup = worn_stat_totals(&snap, &b, &grants, &[2, 8], &plan_active);
        assert_eq!(fx_dedup.get("ATK"), None, "active buff wins; no double count");
        assert_eq!(fx_dedup.get("AC"), Some(&7.0));
        // inactive gear grants nothing
        let fx_inactive = worn_stat_totals(&snap, &b, &grants, &[2], &plan);
        assert_eq!(fx_inactive.get("ATK"), None, "saved-inactive item grants nothing");
    }

    #[test]
    fn out_of_era_source_warns() {
        let mut snap = snapshot();
        snap.items_by_id.get_mut(&1).unwrap().era = Some("Velious".into());
        let mut b = build();
        b.enabled_eras = vec!["Classic".into()]; // Velious not enabled
        b.augments.entry("PRIMARY".into()).or_default().insert("PROC".into(), 1);
        let grants = resolve_augments(&snap, &b);
        assert!(grants["PRIMARY"][0].warnings.iter().any(|w| w.contains("Velious")));
        // empty enabled set = everything allowed -> no era warning
        b.enabled_eras.clear();
        let grants = resolve_augments(&snap, &b);
        assert!(grants["PRIMARY"][0].warnings.is_empty());
    }

    #[test]
    fn extract_min_tier_reads_formula() {
        let mut snap = snapshot();
        assert_eq!(extract_min_tier(&snap), DEFAULT_EXTRACT_MIN_TIER);
        snap.formulas.insert("exaltation_extract_min_tier".into(), "5".into());
        assert_eq!(extract_min_tier(&snap), 5);
    }

    #[test]
    fn sockets_sort_in_window_order() {
        let mut b = build();
        let e = b.augments.entry("PRIMARY".into()).or_default();
        e.insert("PROC".into(), 1);
        e.insert("FOCUS".into(), 3);
        let grants = resolve_augments(&snapshot(), &b);
        let order: Vec<&str> = grants["PRIMARY"].iter().map(|g| g.socket.as_str()).collect();
        assert_eq!(order, vec!["FOCUS", "PROC"], "window order: Focus before Proc");
    }
}
