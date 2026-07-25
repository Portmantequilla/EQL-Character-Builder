//! Character stat assembly (plan §22): base(race, class-mods) + equipment + resolved
//! buffs -> caps + over-cap. Every wiki-absent formula reads from the editable
//! formula table and is confidence-flagged (plan §4.7) — never a hidden constant.
use crate::Snapshot;
use eql_data::{BuffPlan, BuildInput, EquipWarning, StatLine, canonical_slot};
use std::collections::BTreeMap;

pub const ATTRS: [&str; 7] = ["STR", "STA", "AGI", "DEX", "WIS", "INT", "CHA"];
pub const RESISTS: [&str; 5] = ["SV MAGIC", "SV FIRE", "SV COLD", "SV POISON", "SV DISEASE"];

/// Race legality (plan §4.7 stage B). Tokens are race abbreviations plus ALL / NONE /
/// 'except' ("Race: ALL except OGR TRL" -> {ALL, except, OGR, TRL}: the explicit
/// abbreviations are EXCLUSIONS). No build race selected -> legal (skip the check).
pub fn race_legal(item_races: &[String], build_race: Option<&str>) -> bool {
    let Some(race) = build_race else { return true };
    let abbr = race_abbr(race);
    let has = |t: &str| item_races.iter().any(|r| r.eq_ignore_ascii_case(t));
    let listed = has(abbr) || has(race);
    if item_races.is_empty() || has("NONE") {
        true // unrestricted (or beta placeholder)
    } else if has("except") {
        !listed // ALL-except list: explicit races are excluded
    } else if has("ALL") {
        true
    } else {
        listed
    }
}

/// race display name -> the 3-letter token item_races uses (wiki Race: lines)
pub fn race_abbr(race: &str) -> &'static str {
    match race {
        "Barbarian" => "BAR", "Dark Elf" => "DEF", "Dwarf" => "DWF", "Erudite" => "ERU",
        "Froglok" => "FRG", "Gnome" => "GNM", "Half Elf" => "HEF", "Halfling" => "HFL",
        "High Elf" => "HIE", "Human" => "HUM", "Iksar" => "IKS", "Kerra" => "KER",
        "Ogre" => "OGR", "Troll" => "TRL", "Wood Elf" => "ELF",
        _ => "UNKNOWN_RACE",
    }
}

/// Buff Lines statistic label -> our stat key (None = not a stat-block statistic).
/// Wiki labels carry parenthetical suffixes ('HP (Hit Points)', 'Attack (ATK)') —
/// strip them first so future label drift degrades gracefully (verify finding #1).
pub fn statistic_to_stat_key(statistic: &str) -> Option<&'static str> {
    let stripped = statistic.split(" (").next().unwrap_or(statistic).trim();
    match stripped {
        "Strength" => Some("STR"),
        "Stamina" => Some("STA"),
        "Agility" => Some("AGI"),
        "Dexterity" => Some("DEX"),
        "Wisdom" => Some("WIS"),
        "Intelligence" => Some("INT"),
        "Charisma" => Some("CHA"),
        "AC" => Some("AC"),
        "HP" => Some("HP"),
        "Mana" => Some("MANA"),
        "Attack" => Some("ATK"),
        "HP Regeneration" => Some("HP REGEN"),
        "Mana Regeneration" => Some("MANA REGEN"),
        "Magic Resistance" | "Magic" => Some("SV MAGIC"),
        "Fire Resistance" | "Fire" => Some("SV FIRE"),
        "Cold Resistance" | "Cold" => Some("SV COLD"),
        "Poison Resistance" | "Poison" => Some("SV POISON"),
        "Disease Resistance" | "Disease" => Some("SV DISEASE"),
        _ => None, // Haste, Speed, Damage Absorption, Damage Shield: handled elsewhere
    }
}

/// The statistics deliberately NOT summed into the stat block (haste is reported
/// separately; the rest await engine support). The coverage test asserts every
/// distinct DB statistic is either mapped or listed here — nothing silently dropped.
pub const STAT_SKIP_LIST: [&str; 7] = [
    "Haste", "Speed", "Damage Shield", "Damage Shield Stacking",
    "Damage Absorption", "Damage Absorption Magic", "Damage Absorption, Magic",
];

fn fold_stat_key(k: &str) -> &str {
    match k {
        "SV POISION" => "SV POISON", // wiki typo, folded at read time
        other => other,
    }
}

pub struct EquipmentTotals {
    pub stats: BTreeMap<String, i64>,
    /// item-upgrade-tier bonuses, separated for the plan §10 breakdown line
    pub tier_stats: BTreeMap<String, i64>,
    pub best_haste_pct: i64,
    pub warnings: Vec<EquipWarning>,
    /// pageids whose stats actually count (ACTIVE, wearable, level-met)
    pub active_items: Vec<i64>,
}

/// Validate + sum the equipped items. Saved-but-inactive semantics (plan §18):
/// invalid selections KEEP their slot but contribute nothing.
pub fn equipment_totals(snapshot: &Snapshot, build: &BuildInput) -> EquipmentTotals {
    let mut stats: BTreeMap<String, i64> = BTreeMap::new();
    let mut tier_stats: BTreeMap<String, i64> = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut best_haste = 0i64;
    let mut active = Vec::new();
    let classes: Vec<String> = build.classes.iter().map(|c| c.to_uppercase()).collect();

    for (slot, pageid) in &build.equipment {
        let Some(item) = snapshot.items_by_id.get(pageid) else {
            warnings.push(EquipWarning {
                slot: slot.clone(),
                item: format!("item #{pageid}"),
                reason: "item missing from wiki data (DATA_MISSING)".into(),
                status: "SAVED_INACTIVE".into(),
            });
            continue;
        };
        let mut inactive_reason: Option<String> = None;
        let wearable = item.classes.iter().any(|c| c == "ALL")
            || item.classes.iter().any(|ic| classes.iter().any(|c| c.eq_ignore_ascii_case(ic)));
        let race_ok = race_legal(&item.races, build.race.as_deref());
        if !wearable {
            inactive_reason = Some(format!("not wearable by {}", classes.join("/")));
        } else if !race_ok {
            inactive_reason = Some(format!(
                "race-restricted ({})",
                item.races.join("/")
            ));
        } else if !eql_data::era_allowed(item.era.as_deref(), &build.enabled_eras) {
            inactive_reason = Some(format!(
                "out of enabled expansions ({})",
                item.era.as_deref().unwrap_or("?")
            ));
        } else if let Some(req) = item.required_level {
            if (build.level as i64) < req {
                inactive_reason = Some(format!("requires level {req}"));
            }
        }
        // deity restriction: WARN only (build has no deity field yet; plan backlog)
        if inactive_reason.is_none() && !item.deities.is_empty() {
            warnings.push(EquipWarning {
                slot: slot.clone(),
                item: item.name.clone(),
                reason: format!("deity-locked: {}", item.deities.join(", ")),
                status: "ACTIVE".into(),
            });
        }
        let want = canonical_slot(slot);
        if inactive_reason.is_none()
            && want != "ANY" // ANY slots take any class-legal item, no wear-slot check
            && !item.slots.is_empty()
            && !item.slots.iter().any(|s| s == want)
        {
            // wrong slot is a warning, not inactivation: slot data is derived text
            warnings.push(EquipWarning {
                slot: slot.clone(),
                item: item.name.clone(),
                reason: format!("wiki slot says {:?}", item.slots),
                status: "ACTIVE".into(),
            });
        }
        if let Some(reason) = inactive_reason {
            warnings.push(EquipWarning {
                slot: slot.clone(),
                item: item.name.clone(),
                reason,
                status: "SAVED_INACTIVE".into(),
            });
            continue;
        }
        active.push(*pageid);
        let tier = build.equipment_tiers.get(slot).copied().unwrap_or(0).min(10);
        // exact community upgrade rule (item_tier_stat; supersedes the min-+1 pct rule)
        for (k, v) in &item.stats {
            let key = fold_stat_key(k).to_string();
            *stats.entry(key.clone()).or_default() += v;
            let tb = eql_data::item_tier_stat(*v, tier) - v;
            if tb != 0 {
                *tier_stats.entry(key).or_default() += tb;
            }
        }
        if let Some(ac) = item.ac {
            *stats.entry("AC".into()).or_default() += ac;
            let tb = eql_data::item_tier_stat(ac, tier) - ac;
            if tb != 0 {
                *tier_stats.entry("AC".into()).or_default() += tb;
            }
        }
        if let Some(h) = item.haste_pct {
            // worn haste does not stack; best applies — and haste gains +1%/tier
            best_haste = best_haste.max(eql_data::item_tier_haste(h, tier));
        }
    }
    EquipmentTotals { stats, tier_stats, best_haste_pct: best_haste, warnings, active_items: active }
}

// ---- ESTIMATOR base HP/mana model (Mosscovered Legend's EQL Stat Estimator v0.1.4,
// community-measured; formulas extracted from the workbook 2026-07-23 and validated
// vs two live screenshots: mana within ~2-3%, HP ~4-7%). PARTIALLY_VERIFIED.

/// Diminishing STA past 255: ROUND((STA-255)/2)+255 (the workbook's Adjusted STA).
pub fn adjusted_sta(sta: f64) -> f64 {
    if sta > 255.0 { ((sta - 255.0) / 2.0).round() + 255.0 } else { sta }
}

/// The workbook's piecewise INT/WIS-to-mana conversion.
pub fn converted_mana_stat(stat: f64) -> f64 {
    if stat <= 0.0 {
        0.0
    } else if stat <= 100.0 {
        stat
    } else if stat <= 200.0 {
        ((5.0 * stat - 300.0) / 2.0).round()
    } else {
        ((5.0 * ((stat + 200.0) / 2.0).round() - 300.0) / 2.0).round()
    }
}

/// 0 = no mana pool (WAR/MNK/ROG/BER), 1 = INT caster, 2 = WIS caster (workbook Classes).
fn mana_type(class: &str) -> u8 {
    match class {
        "WAR" | "MNK" | "ROG" | "BER" => 0,
        "SHD" | "BRD" | "NEC" | "WIZ" | "MAG" | "ENC" => 1,
        _ => 2, // CLR PAL RNG DRU SHM BST
    }
}

/// Base (HP, mana) from the estimator curves: per-class INT(hp + hp_fac*adjSTA) and
/// INT(mana + mana_fac*convStat), top-2 contributions summed (+5 flat on HP).
/// Returns None when the curve table isn't loaded (fall back to the placeholder).
pub fn estimator_base(
    curve: &BTreeMap<(u32, String), (f64, f64, f64, f64)>,
    classes: &[String],
    level: u32,
    sta: f64,
    wis: f64,
    intel: f64,
) -> Option<(f64, f64)> {
    if curve.is_empty() {
        return None;
    }
    let lvl = level.clamp(1, 100);
    let a = adjusted_sta(sta);
    let (ci, cw) = (converted_mana_stat(intel), converted_mana_stat(wis));
    let mut hp_c: Vec<f64> = Vec::new();
    let mut mana_c: Vec<f64> = Vec::new();
    for c in classes {
        let cu = c.to_uppercase();
        let Some((hp, hpf, mana, manaf)) = curve.get(&(lvl, cu.clone())) else { continue };
        hp_c.push((hp + hpf * a).floor());
        match mana_type(&cu) {
            0 => {} // no pool: contributes nothing to the top-2
            1 => mana_c.push((mana + manaf * ci).floor()),
            _ => mana_c.push((mana + manaf * cw).floor()),
        }
    }
    if hp_c.is_empty() {
        return None;
    }
    hp_c.sort_by(|x, y| y.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal));
    mana_c.sort_by(|x, y| y.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal));
    let hp = 5.0 + hp_c.first().copied().unwrap_or(0.0) + hp_c.get(1).copied().unwrap_or(0.0);
    let mana = mana_c.first().copied().unwrap_or(0.0).max(0.0)
        + mana_c.get(1).copied().unwrap_or(0.0).max(0.0);
    Some((hp, mana))
}

/// PLACEHOLDER base-HP model (thought experiment 2026-07-23, calibrated to two live
/// level-50 screenshots: reference A, 50 CLR/MNK/ENC ≈5395 self-buffed; reference B,
/// 50 SHD/CLR/BRD ≈4094). Structure per the spec: archetype per-level curves blended by
/// multiclass weights w1 ≥ w2 ≥ w3 (defaults 1/1/0 = eqltools' TOP2_SUM), plus a
/// weighted STA return (WAR 6/STA … pure casters 2/STA), linear to 50 with an
/// exponential stub past 50. Every constant is an editable formula row; hp_model=OFF
/// restores the honest base-0. NEEDS_INGAME_TEST throughout — this exists so totals
/// are USEFULLY CLOSE, not authoritative.
pub fn hp_base_placeholder(
    formulas: &BTreeMap<String, String>,
    classes: &[String],
    level: u32,
    sta: f64,
) -> f64 {
    let per_level = parse_class_map(
        formulas.get("hp_per_level_by_class").map(String::as_str).unwrap_or(""),
        &[("WAR", 40.0), ("BER", 35.0), ("MNK", 34.0), ("RNG", 33.0), ("BST", 33.0),
          ("ROG", 32.0), ("BRD", 32.0), ("PAL", 30.0), ("SHD", 30.0), ("CLR", 30.0),
          ("SHM", 30.0), ("DRU", 29.0), ("ENC", 24.0), ("MAG", 24.0), ("NEC", 24.0),
          ("WIZ", 24.0)],
    );
    let sta_coeff = parse_class_map(
        formulas.get("hp_sta_coeff_by_class").map(String::as_str).unwrap_or(""),
        &[("WAR", 6.0), ("PAL", 5.0), ("SHD", 5.0), ("MNK", 4.0), ("RNG", 4.0),
          ("ROG", 4.0), ("BRD", 4.0), ("BST", 4.0), ("BER", 4.0), ("CLR", 3.5),
          ("DRU", 3.5), ("SHM", 3.5), ("ENC", 2.0), ("MAG", 2.0), ("NEC", 2.0),
          ("WIZ", 2.0)],
    );
    let weights: Vec<f64> = formulas
        .get("hp_weights")
        .map(String::as_str)
        .unwrap_or("1.0,1.0,0.0")
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .collect();
    // rank the build's classes by their per-level curve (strongest first), then blend
    let mut ranked: Vec<String> = classes.iter().map(|c| c.to_uppercase()).collect();
    ranked.sort_by(|a, b| {
        per_level.get(b).unwrap_or(&0.0).partial_cmp(per_level.get(a).unwrap_or(&0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let lvl = level.min(50) as f64;
    let mut base = 0.0;
    let mut coeff = 0.0;
    for (i, c) in ranked.iter().take(3).enumerate() {
        let w = weights.get(i).copied().unwrap_or(0.0);
        base += w * per_level.get(c).copied().unwrap_or(0.0) * lvl;
        coeff += w * sta_coeff.get(c).copied().unwrap_or(0.0);
    }
    // phase 2 stub: exponential growth per level past 50 on the class-base term
    if level > 50 {
        let growth: f64 = formulas
            .get("hp_post50_growth")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.10);
        base *= growth.powi((level - 50) as i32);
    }
    base + coeff * sta
}

/// "WAR=40 PAL=33 …" -> map, falling back to the given defaults for missing classes.
fn parse_class_map(text: &str, defaults: &[(&str, f64)]) -> BTreeMap<String, f64> {
    let mut m: BTreeMap<String, f64> =
        defaults.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    for tok in text.split_whitespace() {
        if let Some((k, v)) = tok.split_once('=') {
            if let Ok(x) = v.trim().parse() {
                m.insert(k.trim().to_uppercase(), x);
            }
        }
    }
    m
}

#[cfg(test)]
mod estimator_tests {
    use super::*;

    #[test]
    fn stat_conversions_match_the_workbook() {
        // adjusted STA: linear to 255, then half-rate
        assert_eq!(adjusted_sta(185.0), 185.0);
        assert_eq!(adjusted_sta(255.0), 255.0);
        assert_eq!(adjusted_sta(305.0), 280.0);
        // converted INT/WIS piecewise
        assert_eq!(converted_mana_stat(0.0), 0.0);
        assert_eq!(converted_mana_stat(100.0), 100.0);
        assert_eq!(converted_mana_stat(150.0), 225.0); // (5*150-300)/2
        assert_eq!(converted_mana_stat(171.0), 278.0); // round((5*171-300)/2)=277.5→278
        assert_eq!(converted_mana_stat(250.0), 413.0); // round((5*round(450/2)-300)/2)
    }

    #[test]
    fn estimator_base_top2_and_mana_typing() {
        // synthetic curve @50: CLR hp 1000/fac 3, MNK hp 1200/fac 4 (no mana),
        // ENC hp 800/fac 2 + mana 900/fac 5
        let mut curve = BTreeMap::new();
        curve.insert((50, "CLR".into()), (1000.0, 3.0, 700.0, 4.0));
        curve.insert((50, "MNK".into()), (1200.0, 4.0, 0.0, 0.0));
        curve.insert((50, "ENC".into()), (800.0, 2.0, 900.0, 5.0));
        let classes = vec!["CLR".to_string(), "MNK".into(), "ENC".into()];
        let (hp, mana) = estimator_base(&curve, &classes, 50, 100.0, 150.0, 171.0).unwrap();
        // HP top-2: MNK 1200+400=1600, CLR 1000+300=1300 (+5 flat) — ENC 1000 dropped
        assert_eq!(hp, 5.0 + 1600.0 + 1300.0);
        // mana: MNK excluded (no pool); CLR uses WIS conv(150)=225 → 700+900=1600;
        // ENC uses INT conv(171)=278 → 900+1390=2290; top-2 = both
        assert_eq!(mana, (700.0 + 4.0 * 225.0) + (900.0 + 5.0 * 278.0));
        // empty curve -> None (placeholder fallback)
        assert!(estimator_base(&BTreeMap::new(), &classes, 50, 100.0, 0.0, 0.0).is_none());
    }
}

#[cfg(test)]
mod hp_placeholder_tests {
    use super::*;

    #[test]
    fn anchors_from_the_live_screenshots() {
        let f = BTreeMap::new(); // defaults
        // reference A: 50 CLR/MNK/ENC, in-game STA 185 -> MNK+CLR curves + 7.5/STA
        let ref_a = hp_base_placeholder(
            &f, &["CLR".into(), "MNK".into(), "ENC".into()], 50, 185.0);
        // (MNK 34 + CLR 30) * 50 + (MNK 4 + CLR 3.5) * 185 = 3200 + 1387.5
        assert!((ref_a - 4587.5).abs() < 1.0, "reference A anchor: {ref_a}");
        // reference B: 50 SHD/CLR/BRD, STA 104 -> BRD+SHD? ranked by curve: BRD 32 > SHD 30 = CLR 30
        let ref_b = hp_base_placeholder(
            &f, &["SHD".into(), "CLR".into(), "BRD".into()], 50, 104.0);
        assert!(ref_b > 3800.0 && ref_b < 4100.0, "reference B anchor: {ref_b}");
        // level cap phase: past 50 grows exponentially
        let l55 = hp_base_placeholder(&f, &["WAR".into()], 55, 100.0);
        let l50 = hp_base_placeholder(&f, &["WAR".into()], 50, 100.0);
        assert!(l55 > l50 * 1.3, "post-50 exponential stub engages");
        // single class: only w1 applies
        let solo = hp_base_placeholder(&f, &["WIZ".into()], 50, 60.0);
        assert!((solo - (24.0 * 50.0 + 2.0 * 60.0)).abs() < 1.0);
    }
}

/// Base stats: race base + additive class mods combined per the editable formula
/// (default SUM, NEEDS_INGAME_TEST — plan gap 7).
pub fn base_stats(snapshot: &Snapshot, build: &BuildInput) -> (BTreeMap<String, i64>, String) {
    let mut base: BTreeMap<String, i64> = BTreeMap::new();
    let mut confidence = "PARTIALLY_VERIFIED".to_string();
    match build.race.as_deref().and_then(|r| snapshot.race_base_stats.get(r)) {
        Some(rs) => {
            for (k, v) in rs {
                base.insert(k.clone(), *v);
            }
        }
        None => {
            confidence = "NEEDS_INGAME_TEST (no race selected: base attributes 0)".into();
        }
    }
    let combine = snapshot
        .formulas
        .get("class_attr_combine")
        .map(String::as_str)
        .unwrap_or("SUM");
    for c in &build.classes {
        if let Some(mods) = snapshot.class_stat_mods.get(&c.to_uppercase()) {
            for (k, v) in mods {
                match combine {
                    "BEST_OF" => {
                        let e = base.entry(k.clone()).or_default();
                        *e = (*e).max(*v);
                    }
                    _ => *base.entry(k.clone()).or_default() += v, // SUM default
                }
            }
        }
    }
    (base, confidence)
}

/// Assemble the full stat block. `worn_fx` = flat stat bonuses from WORN item effects
/// + WORN Exaltation grants (augments::worn_stat_totals). Returns (stats map, buff haste %).
pub fn assemble_stats(
    snapshot: &Snapshot,
    build: &BuildInput,
    equip: &EquipmentTotals,
    plan: &BuffPlan,
    worn_fx: &BTreeMap<String, f64>,
) -> (BTreeMap<String, StatLine>, f64) {
    let (base, base_conf) = base_stats(snapshot, build);
    // buff contributions: mutually-stackable lines SUM per statistic (Buff Lines model)
    let mut buff: BTreeMap<&'static str, f64> = BTreeMap::new();
    let mut buff_haste = 0.0f64;
    for l in &plan.lines {
        let Some(c) = &l.chosen else { continue };
        let Some(v) = c.value else { continue };
        match l.statistic.as_deref() {
            Some(s) if s.split(" (").next().unwrap_or(s).trim() == "Haste" => buff_haste += v,
            Some(s) => {
                if let Some(key) = statistic_to_stat_key(s) {
                    *buff.entry(key).or_default() += v;
                }
            }
            None => {}
        }
    }
    // buffed attribute cap (STR..CHA) and the separate resist/save cap. Both are
    // community-reported from the EQL Discord (2026-07-21): 510 attributes, 1000 saves.
    let cap: i64 = snapshot
        .formulas
        .get("stat_cap")
        .and_then(|v| v.parse().ok())
        .unwrap_or(510);
    let resist_cap: i64 = snapshot
        .formulas
        .get("resist_cap")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000);

    let mut out: BTreeMap<String, StatLine> = BTreeMap::new();
    let keys: Vec<&str> = ATTRS
        .iter()
        .chain(["AC", "HP", "MANA", "ATK", "HP REGEN", "MANA REGEN"].iter())
        .chain(RESISTS.iter())
        .copied()
        .collect();
    let hp_model = snapshot
        .formulas
        .get("hp_model")
        .cloned()
        .unwrap_or_else(|| "ESTIMATOR".into());
    let hp_model_on = !hp_model.eq_ignore_ascii_case("OFF");
    let mana_model_on = snapshot
        .formulas
        .get("mana_model")
        .map(|v| !v.eq_ignore_ascii_case("OFF"))
        .unwrap_or(true);
    for key in keys {
        // HP/MANA base models — STA/WIS/INT capped totals are already in `out`
        // because ATTRS precede HP/MANA in the key order. Preference: the community
        // ESTIMATOR curves; hp_model=THOUGHT_EXPERIMENT forces the coefficient
        // placeholder; OFF = honest zero.
        let est = if (key == "HP" && hp_model_on) || (key == "MANA" && mana_model_on) {
            let g = |k: &str| out.get(k).map(|l| l.capped_total).unwrap_or(0.0);
            if hp_model.eq_ignore_ascii_case("THOUGHT_EXPERIMENT") {
                None // explicit opt-out of the estimator curves
            } else {
                estimator_base(&snapshot.class_base_curve, &build.classes, build.level,
                               g("STA"), g("WIS"), g("INT"))
            }
        } else {
            None
        };
        let b = if key == "HP" && hp_model_on {
            match est {
                Some((hp, _)) => hp.round() as i64,
                None => {
                    let sta = out.get("STA").map(|l| l.capped_total).unwrap_or(0.0);
                    hp_base_placeholder(&snapshot.formulas, &build.classes, build.level, sta)
                        .round() as i64
                }
            }
        } else if key == "MANA" && mana_model_on {
            est.map(|(_, m)| m.round() as i64).unwrap_or(0)
        } else {
            *base.get(key).unwrap_or(&0)
        };
        let e = *equip.stats.get(key).unwrap_or(&0);
        let t = *equip.tier_stats.get(key).unwrap_or(&0);
        let ife = *worn_fx.get(key).unwrap_or(&0.0);
        let bf = *buff.get(key).unwrap_or(&0.0);
        let raw = b as f64 + e as f64 + t as f64 + ife + bf;
        let is_attr = ATTRS.contains(&key);
        let is_resist = RESISTS.contains(&key);
        let capped = if is_attr {
            raw.min(cap as f64)
        } else if is_resist {
            raw.min(resist_cap as f64)
        } else {
            raw
        };
        let conf = if (key == "HP" || key == "MANA") && est.is_some() {
            "PARTIALLY_VERIFIED (base from Mosscovered Legend's EQL Stat Estimator \
             curves — community-measured; validated vs live screenshots to ~2-7%; \
             hp_model/mana_model=OFF disables)"
                .to_string()
        } else if key == "HP" && hp_model_on {
            "NEEDS_INGAME_TEST (PLACEHOLDER base-HP model: TOP2-weighted class curves \
             + STA return, calibrated to two live 50 screenshots — editable via the \
             hp_* formulas; hp_model=OFF disables)"
                .to_string()
        } else if key == "HP" || key == "MANA" {
            // combine rule IS known (eqltools client-mined: two highest classes summed,
            // third dropped — formula TOP2_SUM) but the per-class level curves aren't,
            // so base still shows 0 until those are measured
            "NEEDS_INGAME_TEST (per-class HP/mana level curves unknown; rule = two \
             highest classes summed; shows gear+buffs only)"
                .to_string()
        } else if is_attr {
            base_conf.clone()
        } else if key == "ATK" {
            // ATK here is GEAR + BUFFS only. The in-game attack rating also carries a large
            // BASE from STR, the offense skill, and the equipped weapon's skill — for the one
            // live reference (a L50 character: game 432 vs our gear-sum 91) that base is ~341,
            // i.e. most of the number. EQL's attack formula is unpublished and diverges ~3x from
            // the classic EQEmu one (which predicts ~1325 there), so rather than ship a fabricated
            // base we model none yet and say so. Needs in-game calibration — the data to
            // collect is spelled out under "Known incomplete" in AGENTS.md.
            "NEEDS_INGAME_TEST (gear + buffs only — base attack from STR / offense skill / \
             weapon skill is NOT modeled yet; it is most of the real value. EQL's formula is \
             unpublished and ~3x off classic EQ, so it awaits in-game data)"
                .to_string()
        } else {
            "WIKI_CONFIRMED (gear + buff sums)".to_string()
        };
        out.insert(
            key.to_string(),
            StatLine {
                base: b,
                equipment: e,
                tier_bonus: t,
                item_effects: ife,
                buffs: bf,
                raw_total: raw,
                capped_total: capped,
                over_cap: (raw - capped).max(0.0),
                confidence: conf,
            },
        );
    }
    (out, buff_haste)
}
