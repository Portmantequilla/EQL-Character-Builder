// One global build state (Svelte 5 runes). Instantiated once in App.svelte,
// passed to every tab. All mutations go through `build`; App.svelte owns the
// single debounced resolve_build pipeline that fills `result`.
import type { AaAbility, BuildCalculationResult, BuildInput, Item, StaticData } from "./api";

/** Defaults match what is LIVE in the game today (get_static supplies both):
 *  level cap 50, and only the in-era expansions enabled. Both remain adjustable —
 *  the slider goes past the cap and every era can be toggled on. */
export function newBuild(defaults?: { eras?: string[]; levelCap?: number }): BuildInput {
  return {
    name: "New build",
    // start at 1: a new character plans upward from the beginning (the slider still
    // goes to the cap and past it) — was previously seeded at the level cap
    level: 1,
    // fresh start: no classes/race preselected so every install begins empty and the
    // user picks their own trio (was hardcoded SHD/MNK/SHM — the author's own build)
    classes: [],
    race: null,
    // in-era set (empty would mean "everything", incl. unreleased expansions)
    enabled_eras: [...(defaults?.eras ?? [])],
    equipment: {},
    pet_equipment: {},
    equipment_tiers: {},
    spell_tiers: {},
    spellbook: {},
    loadouts: [],
    aa_mnemonic_retention: 0, // legacy; the AA planner (aa_ranks) is the source of truth
    aa_ranks: {},
    aa_points_available: 0,
    disabled_buffs: [],
    strict_buffs: false,
    pet_summon_spell_id: null,
    pet_summon_tier: 0,
    bard_in_group: false,
    augments: {},
    stance: null,
    invocation: null,
    external_buffs: [],
    manual_buffs: [],
    other_buffs: [],
    allow_over_cap: false,
    disabled_lines: [],
    pet_slot_override: null,
  };
}

export class AppState {
  build = $state<BuildInput>(newBuild());
  staticData = $state<StaticData | null>(null);
  result = $state<BuildCalculationResult | null>(null);
  items = $state<Item[]>([]);
  aas = $state<AaAbility[]>([]); // every AA the wiki lists (static; loaded once at startup)
  wishlist = $state<string[]>([]); // Farm tab extra items (client-only)
  resolving = $state(false);
  error = $state<string | null>(null);
  /** bump to force a re-resolve when something OUTSIDE the build changed (formula
   *  edits from Settings or inline controls like the buff-cap slider) */
  resolveNonce = $state(0);

  itemsById = $derived(new Map(this.items.map((i) => [i.pageid, i])));
}
