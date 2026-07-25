//! Shared data types crossing the engine <-> Tauri <-> webview boundary.
//! Plan §2.0.2: one verification vocabulary; §4.2: the immutable result object.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The verification vocabulary (schema CHECKs, engine confidence, UI badges).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verification {
    WikiConfirmed,
    PartiallyVerified,
    NeedsIngameTest,
    ManualOverride,
    LegacyEqData,
    VerifiedIngame,
}

/// A click/worn/focus/proc effect attached to an item (item_effect table),
/// with its own activation level gate (plan §15).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemEffect {
    pub effect_name: String,
    pub activation_type: String, // CLICK | WORN | FOCUS | PROC
    pub required_level: Option<i64>,
    pub spell_id: Option<i64>,
}

/// One item, rich enough for equipping/validation (M2+: slots, stats, level gates).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Item {
    pub pageid: i64,
    pub name: String,
    pub icon_id: Option<i64>, // in-game icon (wiki lucy_img_id); /icons/item_<id>.png
    pub slot: Option<String>,        // raw wiki slot text
    pub slots: Vec<String>,          // canonical paperdoll slots (item_slots table)
    pub classes: Vec<String>,
    pub races: Vec<String>,          // race tokens incl. 'ALL' (item_races table)
    pub deities: Vec<String>,        // deity restriction (item_deity table; empty = none)
    pub ac: Option<i64>,
    pub dmg: Option<i64>,
    pub atk_delay: Option<i64>,
    pub weapon_skill: Option<String>,
    pub haste_pct: Option<i64>,
    pub required_level: Option<i64>,
    pub recommended_level: Option<i64>,
    pub stats: BTreeMap<String, i64>, // STR..CHA, HP, MANA, SV MAGIC...
    pub worn_effect: Option<String>,
    pub focus_effect: Option<String>,
    pub click_effect: Option<String>,
    pub era: Option<String>,
    /// restriction flags as the wiki writes them ("Lore Equipped, Attunable", "NO DROP"…)
    #[serde(default)]
    pub flags: Option<String>,
    #[serde(default)]
    pub weight: Option<f64>,
    #[serde(default)]
    pub size: Option<String>, // TINY | SMALL | MEDIUM | LARGE | GIANT
    /// merchant value as written ("1pp, 1gp, 4sp, 3cp")
    #[serde(default)]
    pub merchant_value: Option<String>,
    /// True only for deliberately non-canonical entries (see
    /// overrides/seeds/supplemental_items.json). Hidden from the pickers and from the
    /// optimizer unless revealed, and never "corrected" against live game data.
    /// Deliberately phrased as the negative so that `Default`, a struct literal, and an
    /// older saved build with no such field all mean "ordinary real item".
    #[serde(default)]
    pub non_canonical: bool,
    /// Class epic quest weapon (overrides/seeds/epic_items.json). The optimizer skips
    /// these unless explicitly allowed — they come from quest chains, not drops — but
    /// they stay selectable by hand in the pickers.
    #[serde(default)]
    pub is_epic: bool,
}

/// The class combo that reveals the non-canonical entries: three Magicians.
/// Documented in CONTRIBUTING.md — this is intentional, not a validation bug.
pub const PICKLE_CLASS: &str = "MAG";

/// True when the build is the all-Magician trio that unlocks non-canonical entries.
pub fn is_pickle_wizard(classes: &[String]) -> bool {
    classes.len() == 3 && classes.iter().all(|c| c.eq_ignore_ascii_case(PICKLE_CLASS))
}

// ---------------------------------------------------------------- buff lines
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuffLineMember {
    pub spell_id: Option<i64>,
    #[serde(default)]
    pub member_name_raw: Option<String>,
    pub source_kind: String, // SPELL | CLICK | PROC | WORN | CONSUMABLE
    #[serde(default)]
    pub value_base: Option<f64>,
    #[serde(default)]
    pub value_max_instrument: Option<f64>,
    #[serde(default)]
    pub source_items: Option<String>,
    #[serde(default, deserialize_with = "int_bool")]
    pub is_group: bool,
    #[serde(default, deserialize_with = "int_bool")]
    pub is_self_only: bool,
}

/// SQLite stores booleans as 0/1 integers; accept both forms.
fn int_bool<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let v: Option<serde_json::Value> = Option::deserialize(d)?;
    Ok(match v {
        Some(serde_json::Value::Bool(b)) => b,
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuffLine {
    pub name: String,
    pub category: String, // 'PET_SEED' lines resolve for target=PET, all others PLAYER
    #[serde(default)]
    pub statistic: Option<String>,
    #[serde(default)]
    pub effect_slot: Option<i64>,
    #[serde(default)]
    pub bard_layer: Option<i64>,
    pub members: Vec<BuffLineMember>,
}

/// How a member is obtainable for a given build (plan engine step 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MemberStatus {
    SelfCast,
    GroupBard, // castable by the group's bard when bard_in_group (M2 refinement)
    Item,
    /// cast on you by ANOTHER PLAYER — the Spells tab's external-buff list. USABLE
    /// (counts in the plan), unlike External which is merely "exists elsewhere".
    ExternalCast,
    External,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    Player,
    Pet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedMember {
    pub name: String,
    pub status: MemberStatus,
    pub why: String,
    pub value: Option<f64>,
    pub self_only: bool,
    pub group: bool,
    pub source_kind: String,
    pub spell_id: Option<i64>,
    /// how this member entered the plan: AUTO (own class pick) | MANUAL (user chose a
    /// specific rank) | OTHER (deliberately enabled item/consumable) | EXTERNAL (cast
    /// by another player) | BARD (group bard) | "" (not applicable / unavailable)
    #[serde(default)]
    pub source_tag: String,
    /// the spell's duration text, for the per-buff display
    #[serde(default)]
    pub duration: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineResolution {
    pub line: String,
    pub statistic: Option<String>,
    pub effect_slot: Option<i64>,
    pub bard_layer: Option<i64>,
    pub chosen: Option<ResolvedMember>,
    pub n_usable: usize,
    pub n_members: usize,
    pub alternatives: Vec<ResolvedMember>,
    /// set when a later stage (combination consumption / buff cap) changed `chosen`
    #[serde(default)]
    pub rejected_reason: Option<String>,
}

/// The resolved buff set after combination consumption + the 15-buff cap (plan §7/§12).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuffPlan {
    pub lines: Vec<LineResolution>,
    pub active: Vec<ActiveBuff>,
    pub rejected: Vec<RejectedBuff>,
    pub buff_slots_used: usize,
    pub buff_slot_cap: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveBuff {
    pub name: String,
    pub lines: Vec<String>,
    pub total_value: f64,
    pub status: MemberStatus,
    pub why: String,
    /// AUTO | MANUAL | OTHER | EXTERNAL | BARD (the Buffs tab's source badge)
    #[serde(default)]
    pub source_tag: String,
    #[serde(default)]
    pub duration: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedBuff {
    pub name: String,
    pub line: String,
    pub reason: String,
}

// ---------------------------------------------------------------- build input
/// A build as the user configures it (persisted to builds.db with soft refs).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildInput {
    pub name: String,
    pub level: u32,
    pub classes: Vec<String>,
    pub race: Option<String>,
    /// paperdoll slot key (EAR1, EAR2, HEAD, ... PRIMARY, SECONDARY, FINGER1, FINGER2, AMMO)
    pub equipment: BTreeMap<String, i64>,
    /// enabled expansions/eras; EMPTY = everything enabled. Items tagged with an era
    /// not in this set go saved-but-inactive (plan §18) and drop out of pickers.
    /// Untagged items (era NULL, ~37% of the mirror) are always allowed.
    #[serde(default)]
    pub enabled_eras: Vec<String>,
    /// pet paperdoll: "PET_HEAD", "PET_PRIMARY", … -> item pageid (give-item model,
    /// Pet Guide). Legacy saves used positional "PET_1".."PET_N" keys — re-homed on
    /// load by `migrate_legacy_pet_keys`.
    #[serde(default)]
    pub pet_equipment: BTreeMap<String, i64>,
    /// item upgrade tier 0..10 per slot key (player paperdoll AND PET_ slots).
    /// Rule (Item Upgrade System page, PARTIALLY_VERIFIED): cumulative +10%/tier,
    /// rounds down, minimum +1 per tier; weapon delay never changes.
    #[serde(default)]
    pub equipment_tiers: BTreeMap<String, u32>,
    /// spell upgrade tier 0..10 per spell pageid (plan naming law: spell_upgrade_tier).
    /// Scaling (community-reconstructed 2026-07, formula-driven): dmg/heal +6%/tier
    /// linear floor (`spell_tier_value`), mana ~-6%/tier PROVISIONAL, cast/reuse
    /// -4%/tier. For pet summon spells the tier ALSO raises the pet: +1 level per tier
    /// (capped one below the character), +6% HP, +1 max hit, +5 skill points per level
    /// (official 7/7 notes).
    #[serde(default)]
    pub spell_tiers: BTreeMap<i64, u32>,
    /// the spellbook: book slot index (0-based; 16 squares per open spread, 8 per
    /// page) -> scribed wiki spell pageid
    #[serde(default)]
    pub spellbook: BTreeMap<u32, i64>,
    /// memorized-spell loadouts, mirroring the game's [SpellLoadouts] INI section
    /// (up to 60 sets of 14 gem slots) — the import/export payload
    #[serde(default)]
    pub loadouts: Vec<SpellLoadout>,
    /// AA: Mnemonic Retention rank 0..6 (kept for builds saved before the AA planner;
    /// `aa_ranks` is now the source of truth and wins when both are set).
    #[serde(default)]
    pub aa_mnemonic_retention: u32,
    /// the AA planner: aa row id -> purchased rank
    #[serde(default)]
    pub aa_ranks: BTreeMap<i64, u32>,
    /// AA points the character has to spend (user-entered; the game has no local file
    /// that records it)
    #[serde(default)]
    pub aa_points_available: u32,
    /// buff names the user toggled OFF on the Buffs tab (CUSTOM mode, plan §3)
    #[serde(default)]
    pub disabled_buffs: Vec<String>,
    /// strict availability: self-cast buffs need their spell SCRIBED in the spellbook;
    /// item buffs need a source item currently equipped (incl. the ANY slots)
    #[serde(default)]
    pub strict_buffs: bool,
    pub pet_summon_spell_id: Option<i64>,
    #[serde(default)]
    pub pet_summon_tier: u32,
    #[serde(default)]
    pub bard_in_group: bool,
    /// manual override for the pet inventory slot count (Some = user set what they see
    /// in game; None = use the data-derived default). Clamped to 1..=PET_SLOT_MAX by the
    /// engine. The derived count is PARTIALLY_VERIFIED, so the user's eyes win.
    #[serde(default)]
    pub pet_slot_override: Option<u32>,
    /// augment sockets: slot key (paperdoll OR pet "PET_<SLOT>") -> socket type -> SOURCE item
    /// pageid. An Exaltation augment IS "<source item> (Exaltation)": the source's
    /// effect (proc/worn/click/focus) transfers to the host item; stats never move.
    /// Extraction requires the source at +4 (user-verified in game 2026-07-15).
    #[serde(default)]
    pub augments: BTreeMap<String, BTreeMap<String, i64>>,
    /// active combat stance id (eqlbuilds stance table; one at a time) — display-only
    /// v1: the descriptions carry the numbers, nothing feeds the stat pipeline yet
    #[serde(default)]
    pub stance: Option<String>,
    /// active invocation id (caster combat mode; one at a time) — display-only v1
    #[serde(default)]
    pub invocation: Option<String>,
    /// spell pageids CAST ON YOU BY OTHER PLAYERS (the Spells tab's power-planner
    /// list). These become USABLE buff-line members (status EXTERNAL_CAST) regardless
    /// of your classes; self-only spells can never be received this way. The buff
    /// slot cap still trims the total plan.
    #[serde(default)]
    pub external_buffs: Vec<i64>,
    /// spell ids the user MANUALLY picked for their buff lines (Add Class Buff /
    /// "use this rank instead") — overrides the line's automatic strongest pick,
    /// shown with a MANUAL badge
    #[serde(default)]
    pub manual_buffs: Vec<i64>,
    /// buff NAMES from item/consumable sources (clickies, worn, procs, potions) the
    /// user deliberately enabled via "Add Other Buff". Item sources NEVER auto-apply.
    #[serde(default)]
    pub other_buffs: Vec<String>,
    /// theorycrafting: skip the buff-slot-cap trim entirely (the cap is still shown)
    #[serde(default)]
    pub allow_over_cap: bool,
    /// buff LINE names the user turned fully OFF (Clear All / the per-buff eye). A
    /// disabled line contributes NO buff — unlike disabled_buffs (by member name),
    /// which only drops a specific rank and lets the next-best refill the line.
    #[serde(default)]
    pub disabled_lines: Vec<String>,
}

/// Spell gems. Base 8 (classic), +1 per rank of the AA **Mnemonic Retention**
/// (6 ranks: "+1/2/3/4/5/6 additional spells", wiki Alternate Advancement page).
/// 8 + 6 = 14 — which is exactly how many slots the game writes per loadout.
pub const BASE_SPELL_GEMS: usize = 8;
pub const MNEMONIC_MAX_RANK: u32 = 6;
pub const MAX_SPELL_GEMS: usize = BASE_SPELL_GEMS + MNEMONIC_MAX_RANK as usize; // 14

/// How many gems this build actually has.
pub fn spell_gem_count(mnemonic_rank: u32) -> usize {
    BASE_SPELL_GEMS + mnemonic_rank.min(MNEMONIC_MAX_RANK) as usize
}

/// One Alternate Advancement ability (wiki Alternate Advancement page).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AaAbility {
    pub id: i64,
    pub name: String,
    pub category: String,          // GENERAL | ARCHETYPE | CLASS | SPECIAL
    pub class_abbr: Option<String>, // CLASS rows only
    pub max_rank: u32,
    /// per-rank costs; None where the wiki wrote "?"
    pub costs: Vec<Option<u32>>,
    /// false = the wiki's cost list has unknowns, so totals are a LOWER BOUND
    pub cost_complete: bool,
    pub required_level: Option<u32>,
    pub description: String,
}

/// What the AA planner reports back.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AaPlan {
    pub points_spent: u32,
    pub points_available: u32,
    /// true when any purchased rank has an unknown ("?") cost on the wiki
    pub cost_is_lower_bound: bool,
    /// AAs whose required_level is above the build's level (kept, flagged)
    pub level_locked: Vec<String>,
    /// purchased AAs the build's classes don't grant (kept, flagged)
    pub class_locked: Vec<String>,
}

/// A memorized-spell loadout — the game's [SpellLoadouts] section (up to 60 sets,
/// 14 gem slots each). NOTE: the file stores only base spell ids; a spell's upgrade
/// tier is server-side state and never appears on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpellLoadout {
    pub name: String,
    /// up to 14 entries; None = empty gem (-1 in the INI)
    pub slots: Vec<Option<i64>>, // wiki spell pageids
}

/// The 23 paperdoll slot keys, in display order. FINGER1/2, EAR1/2, WRIST1/2 map to the
/// canonical item slots FINGER/EAR/WRIST. ANY1/ANY2 accept ANY class-legal item
/// regardless of its wear slot (potions, spare weapons, clickies).
pub const PAPERDOLL_SLOTS: [&str; 23] = [
    "EAR1", "HEAD", "FACE", "EAR2", "NECK", "SHOULDERS", "ARMS", "BACK",
    "WRIST1", "WRIST2", "RANGE", "HANDS", "PRIMARY", "SECONDARY",
    "FINGER1", "FINGER2", "CHEST", "LEGS", "FEET", "WAIST", "AMMO", "ANY1", "ANY2",
];

/// The in-game inventory window arrangement (Equipment Layout reference, 2026-07-20):
/// four stone bars of slots. Shared by the player Equipment tab and the pet paperdoll
/// (pet keys are the same with a "PET_" prefix). Row order here IS the active-slot
/// priority order for pets (first `slot_count` filled wells are active).
pub const PAPERDOLL_ROWS: [&[&str]; 4] = [
    &["EAR1", "NECK", "FACE", "HEAD", "EAR2"],
    &["FINGER1", "WRIST1", "ARMS", "HANDS", "WRIST2", "FINGER2"],
    &["SHOULDERS", "CHEST", "BACK", "WAIST", "LEGS", "FEET"],
    &["PRIMARY", "SECONDARY", "RANGE", "AMMO", "ANY1", "ANY2"],
];

/// The pet's paperdoll keys in row order ("PET_EAR1" …). Pets gear like players; the
/// class-combo slot budget (base 4 + summed class bonuses) limits how many filled
/// slots are ACTIVE, not which slots exist.
pub fn pet_paperdoll_slots() -> Vec<String> {
    PAPERDOLL_ROWS
        .iter()
        .flat_map(|row| row.iter().map(|s| format!("PET_{s}")))
        .collect()
}

/// The known eras in unlock order (display order for the expansion toggles).
pub const ERA_ORDER: [&str; 12] = [
    "Classic", "Fear", "Hate", "Sky", "Temple", "Paineel", "Kunark",
    "Chardok Revamp", "Epic Quests", "Velious", "FearHateRevamp", "Legends Only",
];

/// Eras LIVE IN THE GAME right now — the app's default enabled set.
/// Source: the wiki's own `Template:PageEra` switch (WIKI_CONFIRMED, fetched
/// 2026-07-14), which flags every other era's pages "Out of Era":
///   in  = classic, fear, hate, hole, sky, stonebrunt, temple, warrens, paineel
///   out = kunark, velious, luclin, chardok, chardokrevamp, holevp,
///         warrensfearhaterevamp, fearhaterevamp, epics, epicquests
/// "Legends Only" (EQL-exclusive additions) has no key in that switch, so it falls
/// through to `out` — but it is by definition content this game shipped, so we
/// enable it. PARTIALLY_VERIFIED: revisit if the wiki adds a key for it.
/// Hole/Stonebrunt/Warrens are in-era but currently tag zero items in our mirror.
pub const DEFAULT_ENABLED_ERAS: [&str; 10] = [
    "Classic", "Fear", "Hate", "Hole", "Sky", "Stonebrunt", "Temple", "Warrens",
    "Paineel", "Legends Only",
];

/// Level cap live in the game today (the plan's build spec says 1-50). The UI still
/// allows higher for planning against future caps.
pub const DEFAULT_LEVEL_CAP: u32 = 50;

/// Is an item with this era tag allowed under the build's enabled set?
/// Empty set = everything; untagged (None) items are always allowed.
pub fn era_allowed(item_era: Option<&str>, enabled: &[String]) -> bool {
    match item_era {
        None => true,
        Some(e) => enabled.is_empty() || enabled.iter().any(|x| x == e),
    }
}

/// paperdoll slot key -> canonical item slot ("ANY" = no slot restriction)
pub fn canonical_slot(paperdoll: &str) -> &str {
    match paperdoll {
        "EAR1" | "EAR2" => "EAR",
        "WRIST1" | "WRIST2" => "WRIST",
        "FINGER1" | "FINGER2" => "FINGER",
        "ANY1" | "ANY2" => "ANY",
        s => s,
    }
}

/// Re-home legacy positional pet gear ("PET_1".."PET_N", the pre-paperdoll model) onto
/// the pet paperdoll keys ("PET_HEAD" …). Each item goes to the first free key among
/// its natural wear slots (row order), else PET_ANY1/PET_ANY2, else the first free key
/// anywhere (never silently dropped — at most 12 legacy slots vs 23 wells). The slot's
/// upgrade tier and augment sockets move with it. `item_slots` looks up an item's wear
/// slots by pageid (None/empty = unknown -> the ANY fallback). Returns the moves made
/// as (old key, new key) so callers can log or persist.
pub fn migrate_legacy_pet_keys(
    pet_equipment: &mut BTreeMap<String, i64>,
    equipment_tiers: &mut BTreeMap<String, u32>,
    augments: &mut BTreeMap<String, BTreeMap<String, i64>>,
    item_slots: impl Fn(i64) -> Option<Vec<String>>,
) -> Vec<(String, String)> {
    let legacy: Vec<String> = pet_equipment
        .keys()
        .filter(|k| {
            k.strip_prefix("PET_")
                .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))
        })
        .cloned()
        .collect();
    if legacy.is_empty() {
        return Vec::new();
    }
    let all_keys = pet_paperdoll_slots();
    let mut moves = Vec::new();
    for old in legacy {
        let Some(pid) = pet_equipment.remove(&old) else { continue };
        let wear: Vec<String> = item_slots(pid)
            .unwrap_or_default()
            .iter()
            .map(|s| s.trim().to_ascii_uppercase())
            .collect();
        // natural wells first (row order), then the ANY wells, then anything free
        let natural = all_keys.iter().filter(|k| {
            let c = canonical_slot(k.strip_prefix("PET_").unwrap_or(k));
            c != "ANY" && wear.iter().any(|w| w == c)
        });
        let any = all_keys.iter().filter(|k| k.ends_with("ANY1") || k.ends_with("ANY2"));
        let new_key = natural
            .chain(any)
            .chain(all_keys.iter())
            .find(|k| !pet_equipment.contains_key(*k))
            .cloned();
        if let Some(new_key) = new_key {
            pet_equipment.insert(new_key.clone(), pid);
            if let Some(t) = equipment_tiers.remove(&old) {
                equipment_tiers.insert(new_key.clone(), t);
            }
            if let Some(a) = augments.remove(&old) {
                augments.insert(new_key.clone(), a);
            }
            moves.push((old, new_key));
        }
    }
    moves
}

// ---------------------------------------------------------------- results
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatLine {
    pub base: i64,
    pub equipment: i64,
    /// bonus from item upgrade tiers (plan §10 "Item upgrades" breakdown line)
    pub tier_bonus: i64,
    /// flat stat bonuses from WORN item effects + WORN Exaltation augments (folded
    /// 2026-07-21; effects also active as buffs are skipped — no double count)
    #[serde(default)]
    pub item_effects: f64,
    pub buffs: f64,
    pub raw_total: f64,
    pub capped_total: f64,
    pub over_cap: f64,
    pub confidence: String, // verification note for the BASE component
}

/// LEGACY approximate rule (+pct%/tier, floor, min +1). Retained ONLY for the
/// SPELL-side buff-value scaling in the resolver; ITEM paths use the exact
/// community rule below (item_tier_stat / item_tier_dmg / item_tier_haste).
pub fn tier_bonus(base: i64, tier: u32, pct: f64) -> i64 {
    if base <= 0 || tier == 0 {
        return 0;
    }
    ((base as f64 * pct / 100.0 * tier as f64).floor() as i64).max(tier as i64)
}

// ---------------------------------------------------------- item upgrade rule (exact)
// The community-reverse-engineered item upgrade formulas ("Mosscovered Legend's EQL
// Stat Estimator" Item Estimator sheet, 2026-07-23 — the authors report 100% parity
// with the game). Confirmed against the live Keg Mallet +5 window: dmg 9 -> 13 (the
// old min-+1 rule wrongly said 14) and STA/WIS 3 -> 8. Partial upgrade progress
// (LOG2(2^tier + partial)) is NOT modeled — the app uses whole tiers.

/// Upgraded STAT value (attributes, AC, HP, mana, resists, regen):
/// 0 unchanged; 0<B<=10 -> B+tier; B>10 -> INT(B + ROUND(B*tier)/10);
/// B<0 -> MIN(0, B+tier) (penalties shrink toward 0, never past it).
pub fn item_tier_stat(base: i64, tier: u32) -> i64 {
    let t = tier as i64;
    if tier == 0 || base == 0 {
        base
    } else if base < 0 {
        (base + t).min(0)
    } else if base <= 10 {
        base + t
    } else {
        let b = base as f64;
        (b + (b * tier as f64).round() / 10.0).floor() as i64
    }
}

/// Upgraded weapon DAMAGE: INT(B + ROUND(B*tier)/10) — no flat low-value branch
/// (this is what makes Keg Mallet 9 -> 13 at +5, not 14). Delay never changes.
pub fn item_tier_dmg(base: i64, tier: u32) -> i64 {
    if tier == 0 || base <= 0 {
        return base;
    }
    let b = base as f64;
    (b + (b * tier as f64).round() / 10.0).floor() as i64
}

/// Upgraded worn HASTE%: flat +1 per tier.
pub fn item_tier_haste(base: i64, tier: u32) -> i64 {
    if base > 0 { base + tier as i64 } else { base }
}

// -------------------------------------------------------------- spell tier scaling
// Community-reconstructed rules (2026-07; confidence per component). The percentages
// are formula_table keys so the user can correct them the moment the game proves
// otherwise; these helpers take the pct so callers stay formula-driven.

/// Damage/healing at `tier`: floor(base * (1 + pct/100 * T)) — LINEAR, not compounded.
/// Applies per qualifying component (DD, DoT tick, heal, HoT tick, lifetap, rain wave).
/// HIGH-CONFIDENCE RECONSTRUCTED: Ice Comet 808 -> floor(808*1.30)=1050 matches reports.
pub fn spell_tier_value(base: i64, tier: u32, pct: f64) -> i64 {
    if tier == 0 {
        return base;
    }
    ((base as f64) * (1.0 + pct / 100.0 * tier as f64)).floor() as i64
}

/// Mana cost at `tier`: round(base * (1 - pct/100 * T)), never below zero. PROVISIONAL:
/// min-mana floors exist (a 10-mana spell stayed 10 at T2) — callers must skip bases
/// under the `spell_tier_mana_floor` formula and treat results as approximate.
pub fn spell_tier_mana(base: i64, tier: u32, pct: f64) -> i64 {
    if tier == 0 {
        return base;
    }
    (((base as f64) * (1.0 - pct / 100.0 * tier as f64)).round() as i64).max(0)
}

/// Cast/recovery/reuse seconds at `tier`: base * (1 - pct/100 * T). HIGH-CONFIDENCE
/// from the wiki's own example: 1.50s -> 1.38s at T2 with pct=4.
pub fn spell_tier_time(base: f64, tier: u32, pct: f64) -> f64 {
    if tier == 0 {
        return base;
    }
    (base * (1.0 - pct / 100.0 * tier as f64)).max(0.0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquipWarning {
    pub slot: String,
    pub item: String,
    pub reason: String,
    pub status: String, // ACTIVE | SAVED_INACTIVE
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetSummonInfo {
    pub spell_id: i64,
    pub name: String,
    pub base_pet_level: Option<i64>,
    pub pet_classes: Option<String>, // 'WAR/SHD'
    pub pet_hp: Option<i64>,
    pub pet_max_hit: Option<i64>,
    /// lowest learn level among ALL classes (for availability display)
    pub class_levels: BTreeMap<String, u32>,
    /// Some = level/HP/hit came (at least partly) from the user research workbook's
    /// ESTIMATES (pet_summon_estimate) rather than wiki-tested values; holds the
    /// sheet's confidence label ("Estimated from legacy range", …) for UI badges
    #[serde(default)]
    pub estimate_confidence: Option<String>,
}

/// One pet paperdoll slot after validation (plan §17 badges).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetGearSlot {
    pub slot: String, // "PET_HEAD", "PET_PRIMARY", … (pet paperdoll key)
    pub item_pageid: Option<i64>,
    pub item_name: Option<String>,
    pub icon_id: Option<i64>,
    /// EMPTY | FULLY_ACTIVE | PROC_INACTIVE | INVALID_CLASS | OUT_OF_ERA | OVER_CAP
    pub badge: String,
    pub reason: Option<String>,
    /// Whether this well is inside the class-combo slot budget. Filled wells consume
    /// the budget in row order; extras are OVER_CAP (red). An EMPTY well is active
    /// while budget remains (green = can still accept an item, red = combo is full).
    #[serde(default)]
    pub active: bool,
}

/// Upper bound for the pet inventory slider. Bonuses SUM across classes
/// (VERIFIED_INGAME 2026-07-20: L1 MAG/BST = 10), so the ceiling is the best triple
/// MAG/BST/NEC = 4 + 3 + 3 + 2 = 12. The engine clamps overrides to this.
pub const PET_SLOT_MAX: usize = 12;

// ---------------------------------------------------------------- augments
/// The five augment socket types, in the game's item-window display order (verified
/// against a live Keg Mallet +5 window, 2026-07-15). ORNAMENTATION is cosmetic; the
/// four Exaltation sockets each accept the matching effect kind extracted from a +4
/// source item. Inventory-dump sub-rows map Slot1->ORNAMENTATION, Slot7->FOCUS,
/// Slot8->CLICK, Slot9->WORN, Slot10->PROC (7/7 match on a live character's sockets).
pub const AUGMENT_SOCKETS: [&str; 5] = ["ORNAMENTATION", "FOCUS", "CLICK", "WORN", "PROC"];

/// Socket type -> the label the game's item window uses.
pub fn augment_socket_label(socket: &str) -> &'static str {
    match socket {
        "ORNAMENTATION" => "Ornamentation",
        "FOCUS" => "Focus Exaltation",
        "CLICK" => "Click Exaltation",
        "WORN" => "Worn Exaltation",
        "PROC" => "Proc Exaltation",
        _ => "Augment",
    }
}

/// Socket type -> the game's label for the effect the augment grants on the host
/// ("Combat Effect: Earthquake (Req Level 30)" in the item window).
pub fn augment_effect_label(socket: &str) -> &'static str {
    match socket {
        "PROC" => "Combat Effect",
        "WORN" => "Worn Effect",
        "CLICK" => "Click Effect",
        "FOCUS" => "Focus Effect",
        _ => "Effect",
    }
}

/// One entry in the augment catalog: an effect-bearing item viewed as the Exaltation
/// augment it can become. Regen effects are excluded (not extractable — user report,
/// 2026-07-15). Restrictions (classes / hand slots) come from the SOURCE item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AugmentInfo {
    pub source_pageid: i64,
    /// "<source item> (Exaltation)" — how the game names socketed augments
    pub name: String,
    /// which socket it fits: FOCUS | CLICK | WORN | PROC
    pub socket: String,
    pub effect_name: String,
    pub required_level: Option<i64>,
    /// the source item's class restriction (empty or ["ALL"] = unrestricted)
    pub classes: Vec<String>,
    /// the source item's race restriction (empty or ["ALL"] = unrestricted)
    #[serde(default)]
    pub races: Vec<String>,
    /// the source item's wear slots (a hand-slot source restricts host hand slots)
    pub slots: Vec<String>,
    /// the source item's restriction flags ("No Trade, Placeable" …)
    #[serde(default)]
    pub flags: Option<String>,
    /// the linked effect spell (item_effect.spell_id) for the hover explanation
    #[serde(default)]
    pub spell_id: Option<i64>,
    pub era: Option<String>,
    pub icon_id: Option<i64>,
}

/// One effect a worn item (or its socketed augment) gives the character — the Stats
/// page "what my gear does" list. DISPLAY-ONLY for now: these are not folded into the
/// stat totals until effect formulas + stacking data are collected (plan honesty rule).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemEffectOverview {
    pub kind: String,        // PROC | WORN | FOCUS | CLICK
    pub label: String,       // the game's wording: "Combat Effect" | "Worn Effect" | …
    pub effect_name: String,
    pub source_slot: String, // paperdoll key it comes from
    pub source_item: String, // "Keg Mallet +5"
    /// Some = the effect arrives via a socketed Exaltation, not the item itself
    pub via_augment: Option<String>,
    pub required_level: Option<i64>,
    /// linked spell (item_effect.spell_id) for the hover explanation, when known
    pub spell_id: Option<i64>,
    /// true = required_level is above the build's level (shown, but flagged)
    pub level_gated: bool,
    pub warnings: Vec<String>,
}

/// One socketed augment on an equipped item, after validation. Advisory: rule breaks
/// WARN (the planner never hard-blocks what the user says they did in game).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AugmentGrant {
    /// host slot key (paperdoll or PET_N)
    pub slot: String,
    pub socket: String,
    pub source_pageid: i64,
    pub name: String,          // "<source> (Exaltation)"
    pub effect_name: String,   // what the host now shows (e.g. "Earthquake")
    pub required_level: Option<i64>,
    pub warnings: Vec<String>, // class/slot/extraction rule breaks (advisory)
}

/// A weapon/off-hand the pet was given, after the hand-rule check. Pets wield like a
/// player: one 2H, OR two 1H, OR one 1H + a shield/off-hand. `hand` is None (and
/// `active` false) when the rule leaves no room for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetWeapon {
    pub slot: String,           // PET_N it was given in
    pub item_name: String,
    pub category: String,       // "2H" | "1H" | "SHIELD" | "OFFHAND"
    pub hand: Option<String>,   // "PRIMARY" | "SECONDARY" | None (unwieldable)
    pub active: bool,           // false = given but the hand rule leaves no room
    pub note: Option<String>,   // why it isn't wielded, when active is false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetBlock {
    pub summon: PetSummonInfo,
    /// false = summon no longer castable by this build (saved-but-inactive, plan §18)
    pub valid: bool,
    /// lowest level at which a build class learns the summon, when above build level
    pub becomes_valid_at: Option<u32>,
    /// MIN(base + tier, character level - 1) floored at 1; None = base level unknown
    pub calculated_level: Option<i64>,
    /// effective summon tier: max(pet_summon_tier, spell_tiers[summon spell])
    pub effective_tier: u32,
    /// levels the pet ACTUALLY gained above base (post player-level-1 cap). Official
    /// rule: only these grant stats (+6% HP, +1 base dmg, +5 skill pts each). None =
    /// base level unknown, in which case the tier is used as a fallback with a note.
    #[serde(default)]
    pub levels_gained: Option<u32>,
    /// +6% HP / +1 max hit per ACTUAL gained level (official 7/7 notes)
    pub pet_hp_scaled: Option<i64>,
    pub pet_max_hit_scaled: Option<i64>,
    /// +5 skill points per actual gained level (official; no per-skill model yet)
    #[serde(default)]
    pub skill_point_bonus: u32,
    /// true = some tier ranks were eaten by the player-level-1 cap (granting nothing)
    #[serde(default)]
    pub tier_capped: bool,
    pub intrinsic_classes: Vec<String>,
    /// pet intrinsic classes UNION owner classes (plan §14, PARTIALLY_VERIFIED)
    pub equip_class_pool: Vec<String>,
    /// pet inventory size actually used: the override if set, else default_slot_count
    pub slot_count: usize,
    /// data-derived size: base + best owner-class bonus (Pet Guide, PARTIALLY_VERIFIED).
    /// Shown next to the override control so the user sees where the default came from.
    pub default_slot_count: usize,
    /// true when slot_count comes from the user's manual override, not the derived rule
    pub slot_count_overridden: bool,
    /// slot-bonus contributors for display ("MAG+3 BST+3"); bonuses SUM across classes
    pub slot_bonus_class: Option<String>,
    pub gear: Vec<PetGearSlot>,
    /// summed stats of class-legal given items (proc-inactive items still count stats)
    pub gear_totals: BTreeMap<String, i64>,
    /// the pet's weapons/off-hands after the hand-rule check (1×2H / 2×1H / 1H+shield)
    pub weapon_config: Vec<PetWeapon>,
    /// one-line description of what the pet is wielding when the loadout is legal
    pub weapon_summary: Option<String>,
    /// hand-rule violations (e.g. a 2H plus an off-hand); empty = legal loadout
    pub weapon_warnings: Vec<String>,
    pub buff_lines: Vec<LineResolution>,
    pub notes: Vec<String>,
}

/// The single object every page renders (plan §4.2).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildCalculationResult {
    pub classes: Vec<String>,
    pub level: u32,
    pub race: Option<String>,
    pub wearable_item_count: usize,
    /// stat key -> breakdown. Keys: STR STA AGI DEX WIS INT CHA AC HP MANA ATK,
    /// SV MAGIC/FIRE/COLD/POISON/DISEASE, HP REGEN, MANA REGEN
    pub stats: BTreeMap<String, StatLine>,
    /// best equipped worn-haste % and the haste buff-line value, reported separately
    pub equipment_haste_pct: i64,
    pub buff_haste_pct: f64,
    pub buff_plan: BuffPlan,
    pub pet: Option<PetBlock>,
    pub equipment_warnings: Vec<EquipWarning>,
    /// equipped pageid -> item name (class-filter independent; the UI must NOT resolve
    /// names through the wearable-filtered browser list)
    pub equipped_item_names: BTreeMap<i64, String>,
    /// equipped pageid -> icon id (same independence rule as the names)
    pub equipped_item_icons: BTreeMap<i64, i64>,
    /// memorizable gems: 8 base + Mnemonic Retention rank (max 14)
    pub spell_gem_count: usize,
    /// AA points spent / available + what's locked
    pub aa_plan: AaPlan,
    /// host slot key -> socketed Exaltation augments (validated, warnings advisory)
    pub augment_grants: BTreeMap<String, Vec<AugmentGrant>>,
    /// every effect worn gear + augments give the character (display-only for now)
    pub effect_overview: Vec<ItemEffectOverview>,
    /// build classes whose slot hasn't UNLOCKED at this level (no spells from them;
    /// third slot unlocks at 11 — formula class_3_unlock_level)
    #[serde(default)]
    pub locked_classes: Vec<String>,
    pub notes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pickle_wizard_needs_exactly_three_magicians() {
        let mag3 = vec!["MAG".to_string(), "MAG".to_string(), "MAG".to_string()];
        assert!(is_pickle_wizard(&mag3));
        // case-insensitive, since class strings arrive from saved JSON too
        assert!(is_pickle_wizard(&vec!["mag".into(), "MAG".into(), "Mag".into()]));
        // near misses stay ordinary builds
        assert!(!is_pickle_wizard(&vec!["MAG".into(), "MAG".into()]));
        assert!(!is_pickle_wizard(&vec!["MAG".into(), "MAG".into(), "WAR".into()]));
        assert!(!is_pickle_wizard(&[]));
    }

    #[test]
    fn items_default_to_canonical() {
        // a struct-default Item (and any pre-column DB row) must read as real data
        assert!(!Item::default().non_canonical);
        // a payload saved before the field existed omits it entirely
        let mut v = serde_json::to_value(Item::default()).unwrap();
        v.as_object_mut().unwrap().remove("non_canonical");
        let from_json: Item = serde_json::from_value(v).unwrap();
        assert!(!from_json.non_canonical, "absent flag must mean ordinary data");
    }

    #[test]
    fn pet_paperdoll_mirrors_player_rows() {
        let keys = pet_paperdoll_slots();
        assert_eq!(keys.len(), 23);
        assert_eq!(keys[0], "PET_EAR1");
        assert!(keys.contains(&"PET_PRIMARY".to_string()));
        // every key round-trips to a player paperdoll slot
        for k in &keys {
            let base = k.strip_prefix("PET_").unwrap();
            assert!(PAPERDOLL_SLOTS.contains(&base), "unknown well {k}");
        }
    }

    #[test]
    fn legacy_pet_keys_rehome_by_wear_slot() {
        let mut eq: BTreeMap<String, i64> = BTreeMap::new();
        eq.insert("PET_1".into(), 100); // a HEAD item
        eq.insert("PET_2".into(), 200); // a 2H weapon (PRIMARY)
        eq.insert("PET_3".into(), 300); // slots unknown -> ANY fallback
        eq.insert("PET_HEAD".into(), 999); // already-migrated entry keeps its well
        let mut tiers: BTreeMap<String, u32> = BTreeMap::new();
        tiers.insert("PET_2".into(), 4);
        let mut augs: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
        augs.entry("PET_2".into()).or_default().insert("PROC".into(), 7);
        let moves = migrate_legacy_pet_keys(&mut eq, &mut tiers, &mut augs, |pid| match pid {
            100 => Some(vec!["HEAD".into()]),
            200 => Some(vec!["PRIMARY".into()]),
            _ => None,
        });
        assert_eq!(moves.len(), 3);
        // HEAD well is taken by the migrated entry -> the legacy HEAD item falls to ANY
        assert_eq!(eq.get("PET_HEAD"), Some(&999));
        assert_eq!(eq.get("PET_ANY1"), Some(&100));
        assert_eq!(eq.get("PET_PRIMARY"), Some(&200));
        assert_eq!(eq.get("PET_ANY2"), Some(&300));
        assert!(eq.keys().all(|k| !k.trim_start_matches("PET_").bytes().all(|b| b.is_ascii_digit())));
        // tier + augments moved with the weapon
        assert_eq!(tiers.get("PET_PRIMARY"), Some(&4));
        assert_eq!(augs.get("PET_PRIMARY").and_then(|s| s.get("PROC")), Some(&7));
        // second run is a no-op
        assert!(migrate_legacy_pet_keys(&mut eq, &mut tiers, &mut augs, |_| None).is_empty());
    }

    #[test]
    fn item_tier_rule_matches_the_live_keg_mallet() {
        // the long-open discrepancy, now solved: dmg 9 at +5 = 13 (NOT the old rule's 14)
        assert_eq!(item_tier_dmg(9, 5), 13);
        // low stats are flat +1/tier: STA/WIS 3 -> 8 at +5 (matches the live window)
        assert_eq!(item_tier_stat(3, 5), 8);
        assert_eq!(item_tier_stat(10, 4), 14); // boundary: <=10 stays flat
        // >10 stats: INT(B + ROUND(B*T)/10)
        assert_eq!(item_tier_stat(15, 10), 30);
        assert_eq!(item_tier_stat(11, 1), 12); // INT(11 + 11/10) = 12
        // negatives shrink toward 0, capped
        assert_eq!(item_tier_stat(-5, 3), -2);
        assert_eq!(item_tier_stat(-5, 8), 0);
        assert_eq!(item_tier_stat(0, 7), 0);
        // haste +1%/tier (Cloak of Flames 36 -> 41 at +5); dmg/delay guards
        assert_eq!(item_tier_haste(36, 5), 41);
        assert_eq!(item_tier_dmg(45, 5), 67); // INT(45 + 225/10)
        assert_eq!(item_tier_dmg(9, 0), 9);
    }

    // the user's reference case: Ice Comet base 808 -> floor(808 * 1.30) = 1050 at T5
    #[test]
    fn spell_tier_value_matches_ice_comet() {
        assert_eq!(spell_tier_value(808, 5, 6.0), 1050);
        assert_eq!(spell_tier_value(808, 0, 6.0), 808);
        // linear, NOT compounded: T10 = 808 * 1.60 = 1292.8 -> 1292
        assert_eq!(spell_tier_value(808, 10, 6.0), 1292);
    }

    // the wiki's own timing example: Minor Healing 1.50s -> 1.38s at Tier II (4%/tier)
    #[test]
    fn spell_tier_time_matches_minor_healing() {
        let t2 = spell_tier_time(1.50, 2, 4.0);
        assert!((t2 - 1.38).abs() < 1e-9, "got {t2}");
        // T10 = 60% of original; never negative even at absurd pct
        assert!((spell_tier_time(3.0, 10, 4.0) - 1.8).abs() < 1e-9);
        assert_eq!(spell_tier_time(1.0, 10, 20.0), 0.0);
    }

    // provisional mana rule: 100% -> 70% @T5 -> 40% @T10, rounded, floored at 0
    #[test]
    fn spell_tier_mana_rounds_and_floors() {
        assert_eq!(spell_tier_mana(100, 5, 6.0), 70);
        assert_eq!(spell_tier_mana(100, 10, 6.0), 40);
        assert_eq!(spell_tier_mana(203, 5, 6.0), 142); // Ice Comet: 203 * 0.70 = 142.1
        assert_eq!(spell_tier_mana(10, 10, 20.0), 0);  // clamped, never negative
        assert_eq!(spell_tier_mana(72, 0, 6.0), 72);
    }
}
