//! eql-engine — PURE calculation crate (plan §4.1).
//! Hard rule: NO I/O, NO Tauri/webview imports. Input = an immutable Snapshot the caller
//! loads from wiki.db/builds.db; output = a deterministic BuildCalculationResult (§4.2).
//! M2: buff-line resolution + combination consumption + buff cap, stat assembly,
//! pet resolution, and the seeded "Choose for me" generator.
pub mod aa;
pub mod augments;
pub mod chooser;
pub mod optimizer;
pub mod pet;
pub mod resolver;
pub mod stats;

pub use chooser::choose_for_me;
pub use optimizer::{optimize_gear, optimize_pet_gear, Profile};
pub use resolver::{finish_buff_plan, resolve_buff_lines};

use eql_data::{BuildCalculationResult, BuildInput, Item, PetSummonInfo, Target};
use std::collections::BTreeMap;

/// spell pageid -> { class abbr -> min required level }. BTreeMaps keep iteration
/// (and therefore output) deterministic — plan §4.1 determinism contract.
pub type SpellClassLevels = BTreeMap<i64, BTreeMap<String, u32>>;
/// spell pageid -> display name (wiki page title).
pub type SpellNames = BTreeMap<i64, String>;

/// Immutable inputs the engine reasons over. The Tauri layer builds this from SQLite,
/// keeping the engine free of database concerns (unit-testable + deterministic).
#[derive(Default)]
pub struct Snapshot {
    pub items_by_id: BTreeMap<i64, Item>,
    pub buff_lines: Vec<eql_data::BuffLine>,
    pub class_levels: SpellClassLevels,
    pub spell_names: SpellNames,
    /// race name -> {STR..CHA -> value} (wiki Statistics page)
    pub race_base_stats: BTreeMap<String, BTreeMap<String, i64>>,
    /// class abbr -> additive stat mods (wiki Statistics page)
    pub class_stat_mods: BTreeMap<String, BTreeMap<String, i64>>,
    /// summon spell id -> pet facts (spell_pet_summon)
    pub pet_summons: BTreeMap<i64, PetSummonInfo>,
    /// combination buffs: spell id -> line names it participates in (spell_buff_line)
    pub combinations: BTreeMap<i64, Vec<String>>,
    /// editable formula rows (builds.db formula_table): key -> value_text
    pub formulas: BTreeMap<String, String>,
    /// item pageid -> its click/worn/focus/proc effects with level gates (item_effect)
    pub item_effects: BTreeMap<i64, Vec<eql_data::ItemEffect>>,
    /// pet inventory rule (builds.db pet_equipment_rule): base slots + per-class bonus
    pub pet_slot_rule: PetSlotRule,
    /// the AA table (wiki Alternate Advancement page)
    pub aas: Vec<eql_data::AaAbility>,
    /// spell id -> duration text (per-buff display on the Buffs tab)
    pub spell_durations: BTreeMap<i64, String>,
    /// spell id -> flat stat lines parsed from spell_effect (stat key, amount).
    /// Percent-based rows (haste, movement) are EXCLUDED at load time — worn haste is
    /// already counted via items.haste_pct. Feeds the worn-effect stat fold.
    pub spell_stat_effects: BTreeMap<i64, Vec<(String, f64)>>,
    /// (level, class abbr) -> (hp, hp_fac, mana, mana_fac) — the community base-curve
    /// table (Mosscovered Legend's EQL Stat Estimator; import_stat_estimator.py).
    /// Feeds the ESTIMATOR base HP/mana model. Empty = table not imported.
    pub class_base_curve: BTreeMap<(u32, String), (f64, f64, f64, f64)>,
}

#[derive(Clone)]
pub struct PetSlotRule {
    pub slots_base: usize,
    pub bonus: BTreeMap<String, usize>,
}
impl Default for PetSlotRule {
    fn default() -> Self {
        // Pet Guide seed (PARTIALLY_VERIFIED): base 4; MAG/BST +3, NEC +2, ENC/DRU/SHM +1
        PetSlotRule {
            slots_base: 4,
            bonus: BTreeMap::from([
                ("MAG".into(), 3), ("BST".into(), 3), ("NEC".into(), 2),
                ("ENC".into(), 1), ("DRU".into(), 1), ("SHM".into(), 1), ("SHD".into(), 0),
            ]),
        }
    }
}

/// Availability constraints derived from the build: the scribed spellbook, the
/// currently equipped item names (incl. ANY slots), disabled buffs, strict flag.
pub fn build_constraints(snapshot: &Snapshot, build: &BuildInput) -> resolver::ResolveConstraints {
    resolver::ResolveConstraints {
        strict: build.strict_buffs,
        scribed: build.spellbook.values().copied().collect(),
        equipped_names: build
            .equipment
            .values()
            .filter_map(|pid| snapshot.items_by_id.get(pid))
            .map(|i| i.name.to_lowercase())
            .collect(),
        disabled: build.disabled_buffs.iter().cloned().collect(),
        disabled_lines: build.disabled_lines.iter().cloned().collect(),
        external: build.external_buffs.iter().copied().collect(),
        other: build.other_buffs.iter().map(|n| n.to_lowercase()).collect(),
        manual: build.manual_buffs.iter().copied().collect(),
    }
}

/// Multiclass wearability (plan theorycraft rule): an item is wearable if any of the
/// character's classes appears in its class list, or the item lists ALL.
pub fn item_wearable_by(item: &Item, classes: &[String]) -> bool {
    if item.classes.iter().any(|c| c == "ALL") {
        return true;
    }
    item.classes.iter().any(|ic| classes.iter().any(|c| c.eq_ignore_ascii_case(ic)))
}

/// Which of the build's classes are UNLOCKED at its level (class slots unlock as you
/// level: slot 1 from the start, slots 2/3 at formula-driven levels — the third at 11,
/// user-reported). Locked classes contribute NO spells (buffs, summons, spell lists);
/// gear wearability is deliberately NOT gated here (scope: spells only, per the spec).
pub fn unlocked_classes(snapshot: &Snapshot, build: &BuildInput) -> Vec<String> {
    let f = |key: &str, default: u32| -> u32 {
        snapshot.formulas.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    };
    let thresholds = [1, f("class_2_unlock_level", 1), f("class_3_unlock_level", 11)];
    build
        .classes
        .iter()
        .enumerate()
        .filter(|(i, _)| build.level >= *thresholds.get(*i).unwrap_or(&1))
        .map(|(_, c)| c.to_uppercase())
        .collect()
}

/// The one entry point every page renders from (plan §12 pipeline, M2 slice).
pub fn resolve_build(snapshot: &Snapshot, build: &BuildInput) -> BuildCalculationResult {
    let classes: Vec<String> = build.classes.iter().map(|c| c.to_uppercase()).collect();
    let unlocked = unlocked_classes(snapshot, build);
    let locked_classes: Vec<String> =
        classes.iter().filter(|c| !unlocked.contains(c)).cloned().collect();
    let wearable = snapshot
        .items_by_id
        .values()
        .filter(|i| item_wearable_by(i, &classes))
        .count();

    // equipment validation + totals (saved-but-inactive semantics, plan §18)
    let equip = stats::equipment_totals(snapshot, build);

    // buff plan: per-line strongest -> combination consumption -> buff-cap trim.
    // 6%/tier = the community-reconstructed spell scaling (Ice Comet 808 -> ~1050 @T5)
    let spell_tier_pct: f64 = snapshot
        .formulas
        .get("spell_tier_scaling_pct")
        .and_then(|v| v.parse().ok())
        .unwrap_or(6.0);
    let cons = build_constraints(snapshot, build);
    let mut lines = resolver::resolve_buff_lines_full(
        &snapshot.buff_lines,
        &snapshot.class_levels,
        &snapshot.spell_names,
        &unlocked, // locked class slots contribute no self-cast spells
        build.level,
        Target::Player,
        build.bard_in_group,
        &build.spell_tiers,
        spell_tier_pct,
        &cons,
    );
    // durations for the per-buff display (filled here to keep the resolver signature flat)
    for l in lines.iter_mut() {
        for m in l
            .usables
            .iter_mut()
            .chain(l.res.chosen.iter_mut())
            .chain(l.res.alternatives.iter_mut())
        {
            m.duration = m.spell_id.and_then(|s| snapshot.spell_durations.get(&s).cloned());
        }
    }
    let buff_cap = snapshot
        .formulas
        .get("buff_slot_cap")
        .and_then(|v| v.parse().ok())
        .unwrap_or(resolver::DEFAULT_BUFF_SLOT_CAP);
    let plan = resolver::finish_buff_plan_capped(
        lines,
        &snapshot.combinations,
        buff_cap,
        !build.allow_over_cap, // theorycrafting: report the cap but never trim
    );

    // class-filter-independent name map for the UI (verify finding: never resolve
    // equipped names through the wearable-filtered browser list)
    let equipped_item_names: BTreeMap<i64, String> = build
        .equipment
        .values()
        .filter_map(|pid| snapshot.items_by_id.get(pid).map(|i| (*pid, i.name.clone())))
        .collect();
    let equipped_item_icons: BTreeMap<i64, i64> = build
        .equipment
        .values()
        .filter_map(|pid| {
            snapshot.items_by_id.get(pid).and_then(|i| i.icon_id.map(|ic| (*pid, ic)))
        })
        .collect();

    // augments resolve BEFORE stat assembly so WORN grants can fold into the totals
    let augment_grants = augments::resolve_augments(snapshot, build);
    let worn_fx =
        augments::worn_stat_totals(snapshot, build, &augment_grants, &equip.active_items, &plan);
    let (stat_block, buff_haste) =
        stats::assemble_stats(snapshot, build, &equip, &plan, &worn_fx);
    let pet_block = pet::resolve_pet(snapshot, build);

    let gem_count = aa::spell_gems(snapshot, build);
    let aa_plan = aa::resolve_aa(snapshot, build);
    let effect_overview = augments::effect_overview(snapshot, build, &augment_grants);

    let mut notes = vec![format!(
        "M2 engine: buff resolution WIKI-seeded (verified=0 lines), stat bases {}",
        if build.race.is_some() { "from wiki Statistics page" } else { "unset (pick a race)" }
    )];
    if build.bard_in_group {
        notes.push("assumption: a group bard maintains instrument-scaled songs (IDEAL_SUSTAINED)".into());
    }
    if !locked_classes.is_empty() {
        notes.push(format!(
            "{} locked until higher level — contributing no spells (class slots unlock \
             at formula-driven levels; third at 11, user-reported)",
            locked_classes.join("/")
        ));
    }
    BuildCalculationResult {
        classes,
        level: build.level,
        race: build.race.clone(),
        wearable_item_count: wearable,
        stats: stat_block,
        equipment_haste_pct: equip.best_haste_pct,
        buff_haste_pct: buff_haste,
        buff_plan: plan,
        pet: pet_block,
        equipment_warnings: equip.warnings,
        equipped_item_names,
        equipped_item_icons,
        spell_gem_count: gem_count,
        aa_plan,
        augment_grants,
        effect_overview,
        locked_classes,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eql_data::{BuffLine, BuffLineMember, LineResolution, MemberStatus};

    fn item(pageid: i64, name: &str, classes: &[&str]) -> Item {
        Item {
            pageid,
            name: name.into(),
            classes: classes.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }
    #[test]
    fn wearable_rules() {
        let build = vec!["SHD".to_string(), "MNK".to_string(), "SHM".to_string()];
        assert!(item_wearable_by(&item(1, "a", &["MNK"]), &build));
        assert!(item_wearable_by(&item(2, "b", &["ALL"]), &build));
        assert!(!item_wearable_by(&item(3, "c", &["CLR", "WIZ"]), &build));
    }

    // ---- M2 golden fixtures: the Strength lines from the oracle corpus
    fn spell_member(sid: i64, base: f64, inst: Option<f64>) -> BuffLineMember {
        BuffLineMember { spell_id: Some(sid), member_name_raw: None,
            source_kind: "SPELL".into(), value_base: Some(base),
            value_max_instrument: inst, source_items: None,
            is_group: false, is_self_only: false }
    }
    fn fixture() -> (Vec<BuffLine>, SpellClassLevels, SpellNames) {
        let lines = vec![
            BuffLine { name: "Strength (Primary)".into(), category: "Attribute Enhancing".into(),
                statistic: Some("Strength".into()), effect_slot: None, bard_layer: None,
                members: vec![spell_member(1, 68.0, None),
                              spell_member(2, 67.0, None),
                              spell_member(3, 40.0, None)] },
            BuffLine { name: "Strength (Power)".into(), category: "Attribute Enhancing".into(),
                statistic: Some("Strength".into()), effect_slot: None, bard_layer: None,
                members: vec![spell_member(4, 67.0, None)] },
            BuffLine { name: "Strength (Anthem)".into(), category: "Attribute Enhancing".into(),
                statistic: Some("Strength".into()), effect_slot: None, bard_layer: None,
                members: vec![spell_member(5, 35.0, Some(98.0))] },
            BuffLine { name: "PET_STRENGTH".into(), category: "PET_SEED".into(),
                statistic: Some("Strength".into()), effect_slot: None, bard_layer: None,
                members: vec![BuffLineMember { spell_id: Some(6), member_name_raw: None,
                    source_kind: "SPELL".into(), value_base: None, value_max_instrument: None,
                    source_items: None, is_group: false, is_self_only: false }] },
        ];
        let mut cl: SpellClassLevels = BTreeMap::new();
        cl.insert(1, BTreeMap::from([("SHM".into(), 57)]));
        cl.insert(2, BTreeMap::from([("SHM".into(), 56)]));
        cl.insert(3, BTreeMap::from([("SHM".into(), 44)]));
        cl.insert(4, BTreeMap::from([("SHM".into(), 60)]));
        cl.insert(5, BTreeMap::from([("BRD".into(), 10)]));
        cl.insert(6, BTreeMap::from([("MAG".into(), 11)]));
        let names: SpellNames = BTreeMap::from([
            (1, "Maniacal Strength".into()), (2, "Strength".into()),
            (3, "Storm Strength".into()), (4, "Focus of Spirit".into()),
            (5, "Anthem De Arms".into()), (6, "Burnout".into()),
        ]);
        (lines, cl, names)
    }
    fn plan(build: &[&str], level: u32, target: Target, bard: bool) -> Vec<LineResolution> {
        let (lines, cl, names) = fixture();
        let b: Vec<String> = build.iter().map(|s| s.to_string()).collect();
        resolve_buff_lines(&lines, &cl, &names, &b, level, target, bard)
    }
    fn chosen(res: &[LineResolution], line: &str) -> Option<(String, f64, MemberStatus)> {
        res.iter().find(|r| r.line == line).and_then(|r| {
            r.chosen.as_ref().map(|c| (c.name.clone(), c.value.unwrap(), c.status))
        })
    }

    #[test]
    fn golden_1_strength_primary_maniacal() {
        let p = plan(&["SHD", "MNK", "SHM"], 60, Target::Player, false);
        assert_eq!(chosen(&p, "Strength (Primary)"),
                   Some(("Maniacal Strength".into(), 68.0, MemberStatus::SelfCast)));
    }
    #[test]
    fn golden_2_strength_power_focus_of_spirit() {
        let p = plan(&["SHD", "MNK", "SHM"], 60, Target::Player, false);
        assert_eq!(chosen(&p, "Strength (Power)"),
                   Some(("Focus of Spirit".into(), 67.0, MemberStatus::SelfCast)));
    }
    #[test]
    fn golden_3_anthem_unfillable_without_bard() {
        let p = plan(&["SHD", "MNK", "SHM"], 60, Target::Player, false);
        assert_eq!(chosen(&p, "Strength (Anthem)"), None);
        let r = p.iter().find(|r| r.line == "Strength (Anthem)").unwrap();
        assert_eq!(r.n_members, 1);
        assert_eq!(r.n_usable, 0);
    }
    #[test]
    fn golden_4_level_gating_l56() {
        let p = plan(&["SHM"], 56, Target::Player, false);
        assert_eq!(chosen(&p, "Strength (Primary)"),
                   Some(("Strength".into(), 67.0, MemberStatus::SelfCast)));
    }
    #[test]
    fn golden_5_level_gating_l57() {
        let p = plan(&["SHM"], 57, Target::Player, false);
        assert_eq!(chosen(&p, "Strength (Primary)"),
                   Some(("Maniacal Strength".into(), 68.0, MemberStatus::SelfCast)));
    }
    #[test]
    fn pet_target_filters_to_pet_seed_lines() {
        let p = plan(&["SHD", "MNK", "SHM"], 60, Target::Pet, false);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].line, "PET_STRENGTH");
        assert!(p[0].chosen.is_none());
    }
    #[test]
    fn bard_in_group_scales_instrument_values() {
        let p = plan(&["BRD"], 60, Target::Player, true);
        assert_eq!(chosen(&p, "Strength (Anthem)"),
                   Some(("Anthem De Arms".into(), 98.0, MemberStatus::SelfCast)));
    }
    #[test]
    fn bard_in_group_makes_bard_lines_available() {
        // M2 refinement: SHD/MNK/SHM (no bard) + bard_in_group -> anthem available as GROUP_BARD
        let p = plan(&["SHD", "MNK", "SHM"], 60, Target::Player, true);
        assert_eq!(chosen(&p, "Strength (Anthem)"),
                   Some(("Anthem De Arms".into(), 98.0, MemberStatus::GroupBard)));
    }
    #[test]
    fn third_class_locked_until_11() {
        let snap = Snapshot::default(); // empty formulas -> defaults: slot2@1, slot3@11
        let mut b = BuildInput {
            classes: vec!["WAR".into(), "MNK".into(), "SHM".into()],
            level: 10,
            ..Default::default()
        };
        assert_eq!(unlocked_classes(&snap, &b), vec!["WAR", "MNK"]);
        b.level = 11;
        assert_eq!(unlocked_classes(&snap, &b), vec!["WAR", "MNK", "SHM"]);
        // formula override respected
        let mut snap2 = Snapshot::default();
        snap2.formulas.insert("class_3_unlock_level".into(), "20".into());
        b.level = 15;
        assert_eq!(unlocked_classes(&snap2, &b), vec!["WAR", "MNK"]);
    }

    #[test]
    fn cap_trim_drops_auto_before_deliberate_picks() {
        // 3 lines resolve; cap 2. The MANUAL pin is the WEAKEST buff (Storm Strength
        // 40) — the trim must still drop an AUTO buff first, never the user's pick.
        let (lines, cl, names) = fixture();
        let b = vec!["SHM".to_string()];
        let mut cons = resolver::ResolveConstraints::default();
        cons.manual.insert(3); // Storm Strength (40) pinned for Strength (Primary)
        let res = resolver::resolve_buff_lines_full(&lines, &cl, &names, &b, 60,
            Target::Player, false, &BTreeMap::new(), 6.0, &cons);
        // usable lines: Primary (manual 40) + Power (auto 67); cap 1 forces one drop
        let plan = resolver::finish_buff_plan_capped(res, &BTreeMap::new(), 1, true);
        assert_eq!(plan.active.len(), 1);
        assert_eq!(plan.active[0].name, "Storm Strength", "the pin survives");
        assert_eq!(plan.active[0].source_tag, "MANUAL");
        assert!(plan.rejected.iter().any(|r| r.name == "Focus of Spirit"),
                "the stronger AUTO buff is the one trimmed");
    }

    #[test]
    fn disabled_line_yields_no_buff_and_all_ranks_pinnable() {
        let (lines, cl, names) = fixture();
        let b = vec!["SHM".to_string()];
        // Strength (Primary) has 3 SHM-castable ranks — all must be offered as
        // alternatives (not just the top 3-of-N), so any rank is pinnable
        let base = resolver::resolve_buff_lines_full(&lines, &cl, &names, &b, 60,
            Target::Player, false, &BTreeMap::new(), 6.0,
            &resolver::ResolveConstraints::default());
        let primary = base.iter().find(|r| r.res.line == "Strength (Primary)").unwrap();
        // chosen (Maniacal) + all other usable ranks appear across chosen+alternatives
        let offered = primary.res.alternatives.len()
            + primary.res.chosen.iter().count();
        assert_eq!(offered, primary.res.n_usable, "every usable rank is pinnable");
        // turning the LINE off -> no buff, and it is NOT refilled by a weaker rank
        let mut cons = resolver::ResolveConstraints::default();
        cons.disabled_lines.insert("Strength (Primary)".into());
        let off = resolver::resolve_buff_lines_full(&lines, &cl, &names, &b, 60,
            Target::Player, false, &BTreeMap::new(), 6.0, &cons);
        let primary = off.iter().find(|r| r.res.line == "Strength (Primary)").unwrap();
        assert!(primary.res.chosen.is_none(), "disabled line has no chosen buff");
        assert_eq!(primary.res.rejected_reason.as_deref(), Some("turned off"));
    }

    #[test]
    fn manual_pick_overrides_auto_strongest() {
        // SHM 60 self-casts all three Strength (Primary) ranks; auto picks Maniacal
        // (68). Manually choosing Storm Strength (40, "lower rank for testing") wins
        // the line with a MANUAL tag; removing the pick restores the auto best.
        let (lines, cl, names) = fixture();
        let b = vec!["SHM".to_string()];
        let mut cons = resolver::ResolveConstraints::default();
        cons.manual.insert(3); // Storm Strength
        let res = resolver::resolve_buff_lines_full(&lines, &cl, &names, &b, 60,
            Target::Player, false, &BTreeMap::new(), 6.0, &cons);
        let primary = res.iter().find(|r| r.res.line == "Strength (Primary)").unwrap();
        let c = primary.res.chosen.as_ref().unwrap();
        assert_eq!(c.name, "Storm Strength");
        assert_eq!(c.value, Some(40.0));
        assert_eq!(c.source_tag, "MANUAL");
        assert!(c.why.starts_with("manual pick"));
    }

    #[test]
    fn external_cast_makes_foreign_buffs_usable() {
        // WAR/MNK/ROG can cast NONE of the Strength spells — everything is EXTERNAL
        // and no line resolves. Marking Maniacal Strength (SHM 57) as cast-by-another-
        // player makes exactly that member usable, at full value, status EXTERNAL_CAST.
        let (lines, cl, names) = fixture();
        let b = vec!["WAR".to_string(), "MNK".to_string(), "ROG".to_string()];
        let none = resolver::resolve_buff_lines_full(&lines, &cl, &names, &b, 60,
            Target::Player, false, &BTreeMap::new(), 6.0,
            &resolver::ResolveConstraints::default());
        let primary = none.iter().find(|r| r.res.line == "Strength (Primary)").unwrap();
        assert!(primary.res.chosen.is_none(), "no self path for WAR/MNK/ROG");

        let mut cons = resolver::ResolveConstraints::default();
        cons.external.insert(1); // Maniacal Strength, cast on us by a shaman friend
        let res = resolver::resolve_buff_lines_full(&lines, &cl, &names, &b, 60,
            Target::Player, false, &BTreeMap::new(), 6.0, &cons);
        let primary = res.iter().find(|r| r.res.line == "Strength (Primary)").unwrap();
        let c = primary.res.chosen.as_ref().expect("external cast is usable");
        assert_eq!(c.name, "Maniacal Strength");
        assert_eq!(c.value, Some(68.0));
        assert_eq!(c.status, MemberStatus::ExternalCast);
        assert!(c.why.contains("another player"));
        // spell 2 was NOT marked: still unusable
        assert!(primary.res.alternatives.iter().all(|a| a.name != "Strength"
            || a.status != MemberStatus::ExternalCast));
    }

    #[test]
    fn external_cast_never_receives_self_only_buffs() {
        // a self-only spell lands only on ITS caster — marking it external must not help
        let (mut lines, cl, names) = fixture();
        for m in &mut lines[0].members {
            m.is_self_only = true;
        }
        let b = vec!["WAR".to_string()];
        let mut cons = resolver::ResolveConstraints::default();
        cons.external.insert(1);
        let res = resolver::resolve_buff_lines_full(&lines, &cl, &names, &b, 60,
            Target::Player, false, &BTreeMap::new(), 6.0, &cons);
        let primary = res.iter().find(|r| r.res.line == "Strength (Primary)").unwrap();
        assert!(primary.res.chosen.is_none(), "self-only can't be received externally");
    }

    #[test]
    fn combination_net_loss_is_dropped() {
        // spell 4 (Focus of Spirit, 67 in Power) declared a combination over BOTH
        // Strength lines. Occupying (Primary) would evict Maniacal (68) and provide 0
        // there (not a member) -> net loss -> the §8-lite pass drops the combination
        // and keeps the per-line bests.
        let (lines, cl, names) = fixture();
        let b = vec!["SHM".to_string()];
        let res = resolver::resolve_buff_lines_full(&lines, &cl, &names, &b, 60,
                                                    Target::Player, false, &BTreeMap::new(), 10.0, &resolver::ResolveConstraints::default());
        let combos = BTreeMap::from([(4i64, vec!["Strength (Power)".to_string(),
                                                 "Strength (Primary)".to_string()])]);
        let plan = finish_buff_plan(res, &combos);
        let primary = plan.lines.iter().find(|l| l.line == "Strength (Primary)").unwrap();
        assert_eq!(primary.chosen.as_ref().unwrap().name, "Maniacal Strength");
        let power = plan.lines.iter().find(|l| l.line == "Strength (Power)").unwrap();
        assert!(power.chosen.is_none(), "dropped combo must vacate its held line");
        assert!(power.rejected_reason.as_deref().unwrap().contains("net loss"));
        assert!(plan.rejected.iter().any(|r| r.name == "Focus of Spirit"));
    }
    #[test]
    fn combination_net_gain_displaces_weaker_lines() {
        // give the combo a big second-line presence so occupying both lines is a net
        // WIN -> it must displace the weaker per-line best (classic Aegolism shape)
        let (mut lines, mut cl, mut names) = fixture();
        // make spell 4 also a strong member of Strength (Primary)
        lines[0].members.push(spell_member(4, 66.0, None));
        cl.get_mut(&4).unwrap().insert("SHM".into(), 60);
        names.insert(4, "Focus of Spirit".into());
        let b = vec!["SHM".to_string()];
        let res = resolver::resolve_buff_lines_full(&lines, &cl, &names, &b, 60,
                                                    Target::Player, false, &BTreeMap::new(), 10.0, &resolver::ResolveConstraints::default());
        let combos = BTreeMap::from([(4i64, vec!["Strength (Power)".to_string(),
                                                 "Strength (Primary)".to_string()])]);
        let plan = finish_buff_plan(res, &combos);
        // with = 67 + 66 = 133 > without = 0 + 68 -> combo kept, Maniacal displaced
        let primary = plan.lines.iter().find(|l| l.line == "Strength (Primary)").unwrap();
        assert_eq!(primary.chosen.as_ref().unwrap().name, "Focus of Spirit");
        assert_eq!(primary.chosen.as_ref().unwrap().value, Some(66.0));
        assert!(plan.rejected.iter().any(|r| r.name == "Maniacal Strength"));
        assert_eq!(plan.active.len(), 1);
    }
    #[test]
    fn combination_reinstall_uses_full_usables_not_truncated_alternatives() {
        // combo spell ranks 6th of 7 usables in line B (below the 3-alternative display
        // cut) but is chosen in line A: displacement must still install its line-B value
        let mut lines = vec![BuffLine {
            name: "A".into(), category: "T".into(), statistic: Some("Strength".into()),
            effect_slot: None, bard_layer: None,
            members: vec![spell_member(900, 50.0, None)],
        }];
        let mut members_b: Vec<BuffLineMember> =
            (0..6).map(|i| spell_member(800 + i, 40.0 - i as f64, None)).collect();
        members_b.push(spell_member(900, 12.0, None)); // combo, weakest in B
        lines.push(BuffLine { name: "B".into(), category: "T".into(),
            statistic: Some("Strength".into()), effect_slot: None, bard_layer: None,
            members: members_b });
        let mut cl: SpellClassLevels = BTreeMap::new();
        let mut names: SpellNames = BTreeMap::new();
        for sid in (800..806).chain([900]) {
            cl.insert(sid, BTreeMap::from([("SHM".into(), 1)]));
            names.insert(sid, format!("S{sid}"));
        }
        let res = resolver::resolve_buff_lines_full(&lines, &cl, &names, &["SHM".into()],
                                                    60, Target::Player, false, &BTreeMap::new(), 10.0, &resolver::ResolveConstraints::default());
        let combos = BTreeMap::from([(900i64, vec!["A".to_string(), "B".to_string()])]);
        let plan = finish_buff_plan(res, &combos);
        let b_line = plan.lines.iter().find(|l| l.line == "B").unwrap();
        let c = b_line.chosen.as_ref().expect("combo member must be reinstalled in B");
        assert_eq!(c.name, "S900");
        assert_eq!(c.value, Some(12.0));
    }
    #[test]
    fn overlapping_combo_fully_displaced_loses_claim() {
        // combo X (strong) and combo Y (weak) overlap on both lines; X displaces Y from
        // its only held line, so Y must not displace anything afterwards
        let lines = vec![
            BuffLine { name: "L1".into(), category: "T".into(),
                statistic: Some("Strength".into()), effect_slot: None, bard_layer: None,
                members: vec![spell_member(901, 90.0, None), spell_member(902, 80.0, None)] },
            BuffLine { name: "L2".into(), category: "T".into(),
                statistic: Some("Strength".into()), effect_slot: None, bard_layer: None,
                members: vec![spell_member(902, 70.0, None), spell_member(901, 60.0, None)] },
        ];
        let mut cl: SpellClassLevels = BTreeMap::new();
        let mut names: SpellNames = BTreeMap::new();
        for sid in [901i64, 902] {
            cl.insert(sid, BTreeMap::from([("SHM".into(), 1)]));
            names.insert(sid, format!("S{sid}"));
        }
        let res = resolver::resolve_buff_lines_full(&lines, &cl, &names, &["SHM".into()],
                                                    60, Target::Player, false, &BTreeMap::new(), 10.0, &resolver::ResolveConstraints::default());
        let combos = BTreeMap::from([
            (901i64, vec!["L1".to_string(), "L2".to_string()]),
            (902i64, vec!["L1".to_string(), "L2".to_string()]),
        ]);
        let plan = finish_buff_plan(res, &combos);
        // X=901 holds L1 (90) and displaces 902 from L2, installing its own 60 there;
        // Y=902 then holds nothing and must not displace anything
        let l1 = plan.lines.iter().find(|l| l.line == "L1").unwrap();
        let l2 = plan.lines.iter().find(|l| l.line == "L2").unwrap();
        assert_eq!(l1.chosen.as_ref().unwrap().name, "S901");
        assert_eq!(l2.chosen.as_ref().unwrap().name, "S901");
        assert_eq!(plan.active.len(), 1);
    }
    #[test]
    fn group_bard_never_grants_self_only_songs() {
        // Jonthan's case: a self-only BRD song must NOT become available to a non-bard
        // build via bard_in_group (it only ever lands on the bard itself)
        let lines = vec![BuffLine {
            name: "AC (Layer 2)".into(), category: "T".into(),
            statistic: Some("AC".into()), effect_slot: None, bard_layer: Some(2),
            members: vec![BuffLineMember {
                spell_id: Some(950), member_name_raw: None, source_kind: "SPELL".into(),
                value_base: Some(10.0), value_max_instrument: Some(28.0),
                source_items: None, is_group: false, is_self_only: true,
            }],
        }];
        let cl: SpellClassLevels =
            BTreeMap::from([(950, BTreeMap::from([("BRD".into(), 7u32)]))]);
        let names: SpellNames = BTreeMap::from([(950, "Jonthan's Whistling Warsong".into())]);
        let p = resolve_buff_lines(&lines, &cl, &names,
                                   &["SHD".into(), "MNK".into(), "SHM".into()],
                                   60, Target::Player, true);
        assert!(p[0].chosen.is_none(), "self-only bard song leaked via bard_in_group");
        // ...but a BRD build itself still gets it (SelfCast)
        let p2 = resolve_buff_lines(&lines, &cl, &names, &["BRD".into()],
                                    60, Target::Player, true);
        assert_eq!(p2[0].chosen.as_ref().unwrap().status, MemberStatus::SelfCast);
    }
    #[test]
    fn buff_cap_drops_weakest() {
        // 17 one-member lines -> 15-buff cap drops the 2 weakest
        let mut lines = Vec::new();
        let mut cl: SpellClassLevels = BTreeMap::new();
        let mut names: SpellNames = BTreeMap::new();
        for i in 0..17i64 {
            lines.push(BuffLine {
                name: format!("Line {i:02}"), category: "Test".into(),
                statistic: Some("Strength".into()), effect_slot: None, bard_layer: None,
                members: vec![spell_member(100 + i, 10.0 + i as f64, None)],
            });
            cl.insert(100 + i, BTreeMap::from([("SHM".into(), 1)]));
            names.insert(100 + i, format!("Buff {i:02}"));
        }
        let res = resolver::resolve_buff_lines_full(&lines, &cl, &names, &["SHM".into()], 60,
                                                    Target::Player, false, &BTreeMap::new(), 10.0, &resolver::ResolveConstraints::default());
        // cap is now editable; test the drop logic at an explicit 15
        let plan = resolver::finish_buff_plan_capped(res, &BTreeMap::new(), 15, true);
        assert_eq!(plan.buff_slots_used, 15);
        assert_eq!(plan.buff_slot_cap, 15);
        assert_eq!(plan.rejected.len(), 2);
        // the two weakest (values 10, 11) are the dropped ones
        assert!(plan.rejected.iter().any(|r| r.name == "Buff 00"));
        assert!(plan.rejected.iter().any(|r| r.name == "Buff 01"));
    }
    #[test]
    fn tier_bonus_rule() {
        use eql_data::tier_bonus;
        // cumulative 10%/tier, floored, minimum +1 per tier
        assert_eq!(tier_bonus(100, 0, 10.0), 0);
        assert_eq!(tier_bonus(100, 3, 10.0), 30);
        assert_eq!(tier_bonus(9, 1, 10.0), 1);   // floor(0.9)=0 -> min +1/tier
        assert_eq!(tier_bonus(9, 5, 10.0), 5);   // floor(4.5)=4 -> min +5
        assert_eq!(tier_bonus(25, 4, 10.0), 10);
        assert_eq!(tier_bonus(-5, 8, 10.0), 0);  // negative stats never scale
        assert_eq!(tier_bonus(0, 8, 10.0), 0);
    }
    #[test]
    fn spell_tier_scaling_can_flip_the_strongest_member() {
        // Strength (Primary): Maniacal 68 vs Strength 67 — tier 3 on Strength
        // (67 + max(floor(67*0.3), 3) = 67+20 = 87) must overtake Maniacal
        let (lines, cl, names) = fixture();
        let tiers = BTreeMap::from([(2i64, 3u32)]);
        let res = resolver::resolve_buff_lines_full(
            &lines, &cl, &names, &["SHM".into()], 60, Target::Player, false, &tiers, 10.0, &resolver::ResolveConstraints::default());
        let primary = res.iter().find(|f| f.res.line == "Strength (Primary)").unwrap();
        let c = primary.res.chosen.as_ref().unwrap();
        assert_eq!(c.name, "Strength");
        assert_eq!(c.value, Some(87.0));
        assert!(c.why.contains("tier 3"), "{}", c.why);
    }
    #[test]
    fn determinism_byte_identical() {
        let a = serde_json::to_string(&plan(&["SHD", "MNK", "SHM"], 60, Target::Player, false)).unwrap();
        let b = serde_json::to_string(&plan(&["SHD", "MNK", "SHM"], 60, Target::Player, false)).unwrap();
        assert_eq!(a, b);
    }
    #[test]
    fn chooser_is_deterministic() {
        let mut snap = Snapshot::default();
        for i in 0..40 {
            let mut it = item(i, &format!("Item {i}"), &["ALL"]);
            it.slots = vec!["CHEST".into()];
            it.stats.insert("STR".into(), (i % 7) as i64);
            snap.items_by_id.insert(i, it);
        }
        snap.race_base_stats.insert("Human".into(), BTreeMap::new());
        let a = serde_json::to_string(&choose_for_me(&snap, 42, 60, vec![], vec![])).unwrap();
        let b = serde_json::to_string(&choose_for_me(&snap, 42, 60, vec![], vec![])).unwrap();
        let c = serde_json::to_string(&choose_for_me(&snap, 43, 60, vec![], vec![])).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
