//! Buff-line resolution (plan engine step 9 + M2 refinements: bard-in-group availability,
//! combination-buff consumption, the 15-buff cap). Ported from scripts/resolve_buffs.py
//! and validated against its golden corpus (docs/M2-handoff.md, tests/corpus.rs).
use crate::{SpellClassLevels, SpellNames};
use eql_data::{
    ActiveBuff, BuffLine, BuffLineMember, BuffPlan, LineResolution, MemberStatus,
    RejectedBuff, ResolvedMember, Target,
};
use std::collections::{BTreeMap, BTreeSet};

/// Default buff-slot cap. The wiki's Buff Lines page says 15 (incl. bard songs), but
/// in-game validation shows EQL characters carry MORE than that, so this is only a
/// fallback — the real value is the editable `buff_slot_cap` formula (Settings), and
/// resolve_build passes it in. NEEDS_INGAME_TEST until counted in game.
pub const DEFAULT_BUFF_SLOT_CAP: usize = 30;

/// Availability constraints beyond class/level (CUSTOM mode, plan §3):
/// strict = self-cast buffs require the spell SCRIBED; item buffs require a source
/// item currently equipped. `disabled` = buff names the user toggled off.
#[derive(Default)]
pub struct ResolveConstraints {
    pub strict: bool,
    /// scribed wiki spell ids (the spellbook)
    pub scribed: BTreeSet<i64>,
    /// lowercased names of currently equipped items (incl. the ANY slots)
    pub equipped_names: BTreeSet<String>,
    /// buff MEMBER names toggled off (drops a specific rank; next-best refills the line)
    pub disabled: BTreeSet<String>,
    /// buff LINE names turned fully off (no buff at all — Clear All / the eye)
    pub disabled_lines: BTreeSet<String>,
    /// spell ids cast on you by OTHER PLAYERS (build.external_buffs): usable with
    /// status EXTERNAL_CAST, never gated by strict/scribing (it isn't your book)
    pub external: BTreeSet<i64>,
    /// LOWERCASED buff names from item/consumable sources the user deliberately
    /// enabled (build.other_buffs) — item sources never apply automatically
    pub other: BTreeSet<String>,
    /// spell ids the user manually picked for their lines (build.manual_buffs):
    /// override the automatic strongest-member choice, MANUAL badge
    pub manual: BTreeSet<i64>,
}

impl ResolveConstraints {
    /// does this member's source_items text mention any equipped item?
    fn item_equipped(&self, source_items: Option<&str>) -> bool {
        let Some(src) = source_items else { return false };
        let src = src.to_lowercase();
        self.equipped_names.iter().any(|n| src.contains(n.as_str()))
    }
}

fn availability(
    m: &BuffLineMember,
    build: &[String],
    level: u32,
    class_levels: &SpellClassLevels,
    bard_in_group: bool,
    spell_tiers: &BTreeMap<i64, u32>,
    tier_pct: f64,
    cons: &ResolveConstraints,
) -> (MemberStatus, String, Option<f64>) {
    // bard instrument-scaled value only counts if a bard is in the group to play it
    let eff = match (bard_in_group, m.value_max_instrument) {
        (true, Some(v)) => Some(v),
        _ => m.value_base,
    };
    // spell_upgrade_tier scaling (mirrors the item rule; NEEDS_INGAME_TEST) applies
    // only to spells the build itself casts — item-granted casts are fixed-strength
    let tiered = |v: Option<f64>| -> Option<f64> {
        let tier = m.spell_id.and_then(|sid| spell_tiers.get(&sid)).copied().unwrap_or(0);
        v.map(|x| x + eql_data::tier_bonus(x.round() as i64, tier.min(10), tier_pct) as f64)
    };
    if matches!(m.source_kind.as_str(), "CLICK" | "PROC" | "WORN" | "CONSUMABLE") {
        if cons.strict && !cons.item_equipped(m.source_items.as_deref()) {
            return (
                MemberStatus::External,
                format!("{} item not equipped (strict)", m.source_kind.to_lowercase()),
                eff,
            );
        }
        return (MemberStatus::Item, format!("{} item", m.source_kind.to_lowercase()), eff);
    }
    if let Some(cl) = m.spell_id.and_then(|sid| class_levels.get(&sid)) {
        let scribed_ok =
            !cons.strict || m.spell_id.is_some_and(|sid| cons.scribed.contains(&sid));
        let best = cl
            .iter()
            .filter(|(c, lv)| build.iter().any(|b| b == *c) && **lv <= level)
            .min_by_key(|(c, lv)| (**lv, (*c).clone()));
        if let Some((c, lv)) = best {
            if scribed_ok {
                let tier =
                    m.spell_id.and_then(|sid| spell_tiers.get(&sid)).copied().unwrap_or(0);
                let why = if tier > 0 {
                    format!("cast by {c} (L{lv}, tier {tier})")
                } else {
                    format!("cast by {c} (L{lv})")
                };
                return (MemberStatus::SelfCast, why, tiered(eff));
            }
            // learnable but not scribed: fall through to the item/external paths
        }
        // cast on you by ANOTHER PLAYER (Spells-tab external list): usable regardless
        // of our classes/book — but a self-only buff lands only on ITS caster, never
        // on you. Placed after self-cast so own-casting wins the attribution. The
        // build's spell_tiers entry still applies ("assume the caster has tier N").
        if !m.is_self_only && m.spell_id.is_some_and(|sid| cons.external.contains(&sid)) {
            return (
                MemberStatus::ExternalCast,
                "cast on you by another player".into(),
                tiered(eff),
            );
        }
        // M2 refinement: a bard in the group makes BRD-castable members available —
        // but NEVER self-only songs (Jonthan's line lands only on the bard itself).
        // A group bard's own book is not ours: strict mode does not gate this.
        if bard_in_group && !m.is_self_only {
            if let Some(lv) = cl.get("BRD").filter(|lv| **lv <= level) {
                return (MemberStatus::GroupBard, format!("group bard (L{lv})"), tiered(eff));
            }
        }
    }
    if m.source_items.as_deref().is_some_and(|s| !s.is_empty()) {
        if cons.strict && !cons.item_equipped(m.source_items.as_deref()) {
            return (
                MemberStatus::External,
                "granting item not equipped (strict)".into(),
                eff,
            );
        }
        return (MemberStatus::Item, "item-granted (click/proc/worn)".into(), eff);
    }
    if let Some(cl) = m.spell_id.and_then(|sid| class_levels.get(&sid)) {
        let others: Vec<&str> = cl.keys().map(String::as_str).collect();
        let why = if cons.strict && !m.spell_id.is_some_and(|s| cons.scribed.contains(&s)) {
            format!("not scribed (castable by {})", others.join(","))
        } else {
            format!("only castable by {}", others.join(","))
        };
        return (MemberStatus::External, why, eff);
    }
    (MemberStatus::Unknown, "no class-level data / unresolved".into(), eff)
}

fn usable(status: MemberStatus) -> bool {
    matches!(
        status,
        MemberStatus::SelfCast
            | MemberStatus::GroupBard
            | MemberStatus::Item
            | MemberStatus::ExternalCast
    )
}

/// A line resolution plus the FULL usable list (the public LineResolution truncates
/// alternatives to 3 for display; combination consumption needs every usable member).
pub struct ResolvedLineFull {
    pub res: LineResolution,
    pub usables: Vec<ResolvedMember>,
}

/// Engine step 9: per buff line, the strongest member the build can actually obtain.
/// Deterministic; stable sort keeps member (wiki priority) order on equal values.
/// Compat entry (golden oracle corpus): no spell tiers, default scaling.
pub fn resolve_buff_lines(
    lines: &[BuffLine],
    class_levels: &SpellClassLevels,
    names: &SpellNames,
    build: &[String],
    level: u32,
    target: Target,
    bard_in_group: bool,
) -> Vec<LineResolution> {
    resolve_buff_lines_full(
        lines, class_levels, names, build, level, target, bard_in_group,
        &BTreeMap::new(), 10.0, &ResolveConstraints::default(),
    )
    .into_iter()
    .map(|f| f.res)
    .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_buff_lines_full(
    lines: &[BuffLine],
    class_levels: &SpellClassLevels,
    names: &SpellNames,
    build: &[String],
    level: u32,
    target: Target,
    bard_in_group: bool,
    spell_tiers: &BTreeMap<i64, u32>,
    tier_pct: f64,
    cons: &ResolveConstraints,
) -> Vec<ResolvedLineFull> {
    let build: Vec<String> = build.iter().map(|c| c.to_uppercase()).collect();
    let mut results = Vec::new();
    for line in lines {
        let is_pet = line.category == "PET_SEED";
        if (target == Target::Pet) != is_pet {
            continue;
        }
        let resolved: Vec<ResolvedMember> = line
            .members
            .iter()
            .map(|m| {
                let (status, mut why, value) =
                    availability(m, &build, level, class_levels, bard_in_group,
                                 spell_tiers, tier_pct, cons);
                let name = m
                    .spell_id
                    .and_then(|sid| names.get(&sid).cloned())
                    .or_else(|| m.member_name_raw.clone())
                    .unwrap_or_else(|| format!("spell#{:?}", m.spell_id));
                // item/consumable sources NEVER auto-apply — only names the user
                // deliberately enabled ("Add Other Buff") count; the rest stay
                // visible with an explanatory why (spec §3/§5)
                let item_enabled = status != MemberStatus::Item
                    || cons.other.contains(&name.to_lowercase());
                if status == MemberStatus::Item && !item_enabled {
                    why = format!("{why} — not auto-applied; enable via Add Other Buff");
                }
                let source_tag = match status {
                    MemberStatus::SelfCast => "AUTO",
                    MemberStatus::GroupBard => "BARD",
                    MemberStatus::Item if item_enabled => "OTHER",
                    MemberStatus::ExternalCast => "EXTERNAL",
                    _ => "",
                }
                .to_string();
                ResolvedMember {
                    name,
                    status,
                    why,
                    value,
                    self_only: m.is_self_only,
                    group: m.is_group,
                    source_kind: m.source_kind.clone(),
                    spell_id: m.spell_id,
                    source_tag,
                    duration: None,
                }
            })
            .collect();
        let mut usables: Vec<ResolvedMember> = resolved
            .iter()
            .filter(|a| usable(a.status) && a.value.is_some())
            // item sources: only the deliberately enabled ones (tag OTHER) count
            .filter(|a| a.status != MemberStatus::Item || a.source_tag == "OTHER")
            .filter(|a| !cons.disabled.contains(&a.name)) // user-toggled OFF (Buffs tab)
            .cloned()
            .collect();
        usables.sort_by(|a, b| {
            b.value.partial_cmp(&a.value).unwrap_or(std::cmp::Ordering::Equal)
        });
        // a MANUAL pick overrides the automatic strongest choice for its line — the
        // user chose a specific rank (even a lower one, for testing); best manual wins
        let line_off = cons.disabled_lines.contains(&line.name);
        let chosen = if line_off {
            None
        } else {
            usables
                .iter()
                .filter(|a| a.spell_id.is_some_and(|sid| cons.manual.contains(&sid)))
                .max_by(|a, b| a.value.partial_cmp(&b.value).unwrap_or(std::cmp::Ordering::Equal))
                .cloned()
                .map(|mut m| {
                    m.source_tag = "MANUAL".into();
                    m.why = format!("manual pick — {}", m.why);
                    m
                })
                .or_else(|| usables.first().cloned())
        };
        let n_usable = usables.len();
        // ALL usable ranks (minus the current pick) are pinnable via Add Class Buff —
        // not just the top 3 (the UI slices for its own display)
        let chosen_sid = chosen.as_ref().and_then(|c| c.spell_id);
        let alternatives: Vec<ResolvedMember> = usables
            .iter()
            .filter(|a| chosen_sid.is_none() || a.spell_id != chosen_sid)
            .cloned()
            .collect();
        results.push(ResolvedLineFull {
            res: LineResolution {
                line: line.name.clone(),
                statistic: line.statistic.clone(),
                effect_slot: line.effect_slot,
                bard_layer: line.bard_layer,
                chosen,
                n_usable,
                n_members: resolved.len(),
                alternatives,
                rejected_reason: if line_off { Some("turned off".into()) } else { None },
            },
            usables,
        });
    }
    results
}

/// Steps 10-11 of the M2 resolver: combination consumption + the buff-slot cap.
/// `combinations`: spell_id -> the line names it participates in (spell_buff_line rows).
pub fn finish_buff_plan(
    full: Vec<ResolvedLineFull>,
    combinations: &BTreeMap<i64, Vec<String>>,
) -> BuffPlan {
    finish_buff_plan_capped(full, combinations, DEFAULT_BUFF_SLOT_CAP, true)
}

/// `enforce_cap: false` = theorycrafting over-cap mode: the cap is still REPORTED
/// (header shows N / cap) but nothing is trimmed.
pub fn finish_buff_plan_capped(
    full: Vec<ResolvedLineFull>,
    combinations: &BTreeMap<i64, Vec<String>>,
    cap: usize,
    enforce_cap: bool,
) -> BuffPlan {
    let cap = cap.max(1);
    let mut rejected: Vec<RejectedBuff> = Vec::new();
    let usables_by_line: BTreeMap<String, Vec<ResolvedMember>> = full
        .iter()
        .map(|f| (f.res.line.clone(), f.usables.clone()))
        .collect();
    let mut lines: Vec<LineResolution> = full.into_iter().map(|f| f.res).collect();

    // ---- combination consumption (plan §6 CONSUMES_LINE, deterministic).
    // A chosen spell participating in several lines occupies ALL of them: a DIFFERENT
    // spell chosen in those lines is displaced; the combination's own member value in
    // that line applies instead. Combos process strongest-first (sum of chosen values,
    // spell_id tie-break) and must still HOLD >=1 line when their turn comes — an
    // earlier combo can fully displace a later one (overlapping-combo rule).
    let mut combo_order: Vec<(f64, i64, String)> = {
        let mut m: BTreeMap<i64, (f64, String)> = BTreeMap::new();
        for l in &lines {
            if let Some(c) = &l.chosen {
                if let Some(sid) = c.spell_id {
                    if combinations.get(&sid).is_some_and(|ls| ls.len() > 1) {
                        let e = m.entry(sid).or_insert((0.0, c.name.clone()));
                        e.0 += c.value.unwrap_or(0.0).abs();
                    }
                }
            }
        }
        m.into_iter().map(|(sid, (v, n))| (v, sid, n)).collect()
    };
    combo_order.sort_by(|a, b| {
        b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1))
    });
    let mut banned: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for (_, combo_sid, combo_name) in &combo_order {
        if banned.contains(combo_sid) {
            continue;
        }
        // §8-lite optimizer decision: a combination physically occupies ALL its lines,
        // so keep it only if its summed line values beat the per-line bests it evicts
        // (Arch Shielding's +130 HP must not evict Focus of Spirit's +405).
        let combo_lines = &combinations[combo_sid];
        let member_val = |line: &str, want_combo: bool| -> f64 {
            usables_by_line
                .get(line)
                .and_then(|us| {
                    us.iter().find(|a| {
                        let is_combo = a.spell_id == Some(*combo_sid);
                        let is_banned =
                            a.spell_id.map_or(false, |x| banned.contains(&x));
                        if want_combo { is_combo } else { !is_combo && !is_banned }
                    })
                })
                .and_then(|a| a.value)
                .unwrap_or(0.0)
                .abs()
        };
        let with_combo: f64 = combo_lines.iter().map(|ln| member_val(ln, true)).sum();
        let without: f64 = combo_lines.iter().map(|ln| member_val(ln, false)).sum();
        if without > with_combo {
            banned.insert(*combo_sid);
            for l in lines.iter_mut() {
                if l.chosen.as_ref().and_then(|c| c.spell_id) == Some(*combo_sid) {
                    rejected.push(RejectedBuff {
                        name: combo_name.clone(),
                        line: l.line.clone(),
                        reason: format!(
                            "combination dropped: occupying its lines costs more \
                             ({without:.0}) than it provides ({with_combo:.0})"
                        ),
                    });
                    l.chosen = usables_by_line.get(&l.line).and_then(|us| {
                        us.iter()
                            .find(|a| {
                                a.spell_id != Some(*combo_sid)
                                    && !a.spell_id.map_or(false, |x| banned.contains(&x))
                            })
                            .cloned()
                    });
                    l.rejected_reason = Some(format!("{combo_name} dropped (net loss)"));
                }
            }
            continue;
        }
        // overlapping-combo rule: displaced-everywhere combos lose their claim
        let still_holds = lines.iter().any(|l| {
            l.chosen.as_ref().and_then(|c| c.spell_id) == Some(*combo_sid)
        });
        if !still_holds {
            continue;
        }
        for line_name in combo_lines {
            let Some(l) = lines.iter_mut().find(|l| &l.line == line_name) else { continue };
            let already = l.chosen.as_ref().and_then(|c| c.spell_id) == Some(*combo_sid);
            if already {
                continue;
            }
            if let Some(displaced) = l.chosen.take() {
                rejected.push(RejectedBuff {
                    name: displaced.name.clone(),
                    line: l.line.clone(),
                    reason: format!("blocked by combination buff {combo_name}"),
                });
                // the combination's own member in this line (searched over the FULL
                // usable list, not the display-truncated alternatives) applies instead
                l.chosen = usables_by_line
                    .get(line_name)
                    .and_then(|us| us.iter().find(|a| a.spell_id == Some(*combo_sid)))
                    .cloned();
                l.rejected_reason =
                    Some(format!("{} displaced by {combo_name}", displaced.name));
            }
        }
    }

    // ---- buff cap incl. bard songs (formula buff_slot_cap; wiki said 15, live game
    // differs — the cap is user-settable). Distinct buffs by name.
    let mut distinct: BTreeMap<String, (f64, MemberStatus, String, Vec<String>, String, Option<String>)> =
        BTreeMap::new();
    for l in &lines {
        if let Some(c) = &l.chosen {
            let e = distinct.entry(c.name.clone()).or_insert((
                0.0, c.status, c.why.clone(), vec![],
                c.source_tag.clone(), c.duration.clone(),
            ));
            e.0 += c.value.unwrap_or(0.0).abs();
            e.3.push(l.line.clone());
        }
    }
    // trim order: DELIBERATE picks (manual pins, enabled items, external casts) are
    // dropped LAST — a user's explicit choice must not silently vanish under the cap
    // while an automatic pick survives. Within each group: weakest first, name desc.
    let deliberate = |tag: &str| matches!(tag, "MANUAL" | "OTHER" | "EXTERNAL");
    let mut ordered: Vec<(String, f64, bool)> = distinct
        .iter()
        .map(|(n, (v, _, _, _, tag, _))| (n.clone(), *v, deliberate(tag)))
        .collect();
    ordered.sort_by(|a, b| {
        a.2.cmp(&b.2) // automatic (false) before deliberate (true)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .then(b.0.cmp(&a.0))
    });
    let mut dropped: Vec<String> = Vec::new();
    while enforce_cap && distinct.len() - dropped.len() > cap {
        if let Some((name, _, _)) = ordered.get(dropped.len()) {
            dropped.push(name.clone());
        } else {
            break;
        }
    }
    for name in &dropped {
        for l in lines.iter_mut() {
            if l.chosen.as_ref().map(|c| &c.name) == Some(name) {
                l.chosen = None;
                l.rejected_reason = Some(format!("{cap}-buff cap"));
                rejected.push(RejectedBuff {
                    name: name.clone(),
                    line: l.line.clone(),
                    reason: format!("{cap}-buff cap (weakest total value dropped)"),
                });
            }
        }
        distinct.remove(name);
    }

    let active: Vec<ActiveBuff> = distinct
        .into_iter()
        .map(|(name, (v, status, why, ls, source_tag, duration))| ActiveBuff {
            name,
            lines: ls,
            total_value: v,
            status,
            why,
            source_tag,
            duration,
        })
        .collect();
    let used = active.len();
    BuffPlan {
        lines,
        active,
        rejected,
        buff_slots_used: used,
        buff_slot_cap: cap,
    }
}
