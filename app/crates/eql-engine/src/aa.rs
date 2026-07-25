//! Alternate Advancement planning (wiki "Alternate Advancement" page).
//!
//! Availability: General = every class; Class = only if you took that class;
//! Archetype = "depends on your class combo", but the wiki does NOT say which combo
//! grants which — so we show them all and flag that gap rather than invent a mapping.
//! Special = its own tab.
//!
//! Costs: the wiki's cost lists contain "?" for unfinished entries. A plan touching
//! any of those reports `cost_is_lower_bound`, so the UI never implies false precision.
use crate::Snapshot;
use eql_data::{AaPlan, BuildInput};

/// Is this AA offered to the build at all? (class gating only — level is reported,
/// not hidden, so a plan can look ahead.)
pub fn aa_available(aa: &eql_data::AaAbility, classes: &[String]) -> bool {
    match aa.category.as_str() {
        "CLASS" => aa
            .class_abbr
            .as_deref()
            .is_some_and(|c| classes.iter().any(|b| b.eq_ignore_ascii_case(c))),
        // GENERAL / SPECIAL: everyone. ARCHETYPE: the wiki doesn't publish the
        // class->archetype-AA mapping, so we can't filter it honestly.
        _ => true,
    }
}

/// Cost of taking `rank` ranks (ranks are cumulative: sum of the first N entries).
/// Returns (points, all_costs_known).
pub fn rank_cost(aa: &eql_data::AaAbility, rank: u32) -> (u32, bool) {
    let mut sum = 0;
    let mut known = true;
    for i in 0..rank.min(aa.max_rank) as usize {
        match aa.costs.get(i) {
            Some(Some(c)) => sum += c,
            _ => known = false, // wiki wrote "?" — unknowable, not free
        }
    }
    (sum, known)
}

pub fn resolve_aa(snapshot: &Snapshot, build: &BuildInput) -> AaPlan {
    let classes: Vec<String> = build.classes.iter().map(|c| c.to_uppercase()).collect();
    let mut plan = AaPlan {
        points_available: build.aa_points_available,
        ..Default::default()
    };
    for (id, rank) in &build.aa_ranks {
        let Some(aa) = snapshot.aas.iter().find(|a| a.id == *id) else { continue };
        let rank = (*rank).min(aa.max_rank);
        if rank == 0 {
            continue;
        }
        let (cost, known) = rank_cost(aa, rank);
        plan.points_spent += cost;
        if !known {
            plan.cost_is_lower_bound = true;
        }
        if aa.required_level.is_some_and(|lv| lv > build.level) {
            plan.level_locked.push(format!(
                "{} (needs level {})",
                aa.name,
                aa.required_level.unwrap_or(0)
            ));
        }
        if !aa_available(aa, &classes) {
            plan.class_locked.push(aa.name.clone());
        }
    }
    plan
}

/// Spell gems = 8 base + rank of the AA "Mnemonic Retention" (6 ranks max -> 14).
/// Reads the planner first, then the legacy standalone field (older saved builds).
pub fn spell_gems(snapshot: &Snapshot, build: &BuildInput) -> usize {
    let planned = snapshot
        .aas
        .iter()
        .find(|a| a.name == "Mnemonic Retention")
        .and_then(|a| build.aa_ranks.get(&a.id).copied())
        .unwrap_or(0);
    eql_data::spell_gem_count(planned.max(build.aa_mnemonic_retention))
}
