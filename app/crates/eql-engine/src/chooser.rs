//! "Choose for me" (plan §6 of the final doc): a seeded, deterministic random build.
//! Same seed -> byte-identical build (shareable). No external RNG dependency:
//! SplitMix64, the canonical 64-bit mixer.
use crate::{pet, Snapshot};
use eql_data::{BuildInput, canonical_slot, Item, PAPERDOLL_SLOTS};

pub struct SplitMix64(pub u64);
impl SplitMix64 {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    pub fn pick(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn item_score(item: &Item) -> i64 {
    let stat_sum: i64 = item.stats.values().sum();
    stat_sum + item.ac.unwrap_or(0) * 2 + item.dmg.unwrap_or(0) * 3
}

/// Generate a complete legal build. `classes`: pass empty to roll 3 random classes.
/// `enabled_eras`: empty = all expansions (the roll respects the build's era toggle).
pub fn choose_for_me(
    snapshot: &Snapshot,
    seed: u64,
    level: u32,
    mut classes: Vec<String>,
    enabled_eras: Vec<String>,
) -> BuildInput {
    let mut rng = SplitMix64(seed);
    const ALL: [&str; 16] = ["WAR", "CLR", "PAL", "RNG", "SHD", "DRU", "MNK", "BRD", "ROG",
                             "SHM", "NEC", "WIZ", "MAG", "ENC", "BST", "BER"];
    while classes.len() < 3 {
        let c = ALL[rng.pick(ALL.len())].to_string();
        if !classes.contains(&c) {
            classes.push(c);
        }
    }
    // race: uniform over the imported races (no legality matrix yet — plan backlog V17)
    let races: Vec<&String> = snapshot.race_base_stats.keys().collect();
    let race = if races.is_empty() { None } else { Some(races[rng.pick(races.len())].clone()) };

    let mut build = BuildInput {
        name: format!("Random build #{seed}"),
        level,
        classes: classes.clone(),
        race,
        enabled_eras,
        equipment: Default::default(),
        pet_equipment: Default::default(),
        equipment_tiers: Default::default(),
        spell_tiers: Default::default(),
        spellbook: Default::default(),
        loadouts: Default::default(),
        aa_mnemonic_retention: 0,
        aa_ranks: Default::default(),
        aa_points_available: 0,
        disabled_buffs: Default::default(),
        strict_buffs: false,
        pet_summon_spell_id: None,
        pet_summon_tier: 0,
        bard_in_group: classes.iter().any(|c| c == "BRD"),
        pet_slot_override: None,
        augments: Default::default(),
        stance: None,
        invocation: None,
        external_buffs: Default::default(),
        manual_buffs: Default::default(),
        other_buffs: Default::default(),
        allow_over_cap: false,
        disabled_lines: Default::default(),
    };

    let classes_up: Vec<String> = classes.iter().map(|c| c.to_uppercase()).collect();
    let mut primary_is_2h = false;
    for slot_key in PAPERDOLL_SLOTS {
        if slot_key == "SECONDARY" && primary_is_2h {
            continue;
        }
        let want = canonical_slot(slot_key);
        let mut candidates: Vec<&Item> = snapshot
            .items_by_id
            .values()
            .filter(|i| i.slots.iter().any(|s| s == want))
            .filter(|i| {
                i.classes.iter().any(|c| c == "ALL")
                    || i.classes.iter().any(|ic| classes_up.iter().any(|c| c == ic))
            })
            .filter(|i| crate::stats::race_legal(&i.races, build.race.as_deref()))
            .filter(|i| eql_data::era_allowed(i.era.as_deref(), &build.enabled_eras))
            .filter(|i| i.required_level.map_or(true, |r| r <= level as i64))
            .collect();
        if candidates.is_empty() {
            continue;
        }
        candidates.sort_by(|a, b| item_score(b).cmp(&item_score(a)).then(a.pageid.cmp(&b.pageid)));
        candidates.truncate(12);
        // triangular weighting toward the top: pick min of two uniform draws
        let idx = rng.pick(candidates.len()).min(rng.pick(candidates.len()));
        let item = candidates[idx];
        build.equipment.insert(slot_key.to_string(), item.pageid);
        if slot_key == "PRIMARY" {
            primary_is_2h = item
                .weapon_skill
                .as_deref()
                .is_some_and(|w| w.to_uppercase().starts_with("2H"));
        }
    }
    // pet: strongest summon castable by the build (if any summoner class present)
    build.pet_summon_spell_id = pet::available_summons(snapshot, &build).first().copied();
    build
}
