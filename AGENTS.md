# AGENTS.md — architecture, formulas, and traps

Context for anyone working on this codebase, human or AI. **Read this before changing the
engine.** Several rules here are non-obvious and cost real time to rediscover.

If you are an AI assistant: this file is the ground truth for how the math works. Prefer it over
inference. Where a value is marked unverified, keep it marked — do not "clean up" hedged
labels into confident ones, and never invent a game value.

## What this is

A desktop character planner for **EverQuest Legends**. Plan a 1–3 class character: gear and
upgrade tiers, buffs, spells, spellbook/loadouts, pets, AA, stats, loot filters, macros.
Offline-first — the only network touch is opening wiki links in the user's browser.

## Architecture

Tauri v2 + Svelte 5 (runes) + a Rust workspace under `app/`:

- **`crates/eql-engine`** — PURE calculation. No I/O, no Tauri. Stat assembly, buff resolver,
  optimizer, pet, augments, spell/item tier math. Deterministic and unit-tested. Keep it pure.
- **`crates/eql-data`** — shared types and pure helpers (tier rules, slot maps).
- **`src-tauri`** — SQLite load + Tauri commands. **The only I/O layer.**
- **`app/src`** — Svelte UI. Tabs in `app/src/lib/tabs/`, command contracts in `lib/api.ts`,
  TS mirrors of engine math in `lib/format.ts`. The build-wide class/race/level picker lives in
  `App.svelte`, not in a tab.

**Two databases.** `wiki.db` is a disposable game-data mirror. `builds.db` holds per-user saves
plus an editable `formula_table` and lives in `%LOCALAPPDATA%/EQLBuilder` — it is the precious
one. `migrations/builds.sql` is the only file compiled into the binary (`include_str!`); it
seeds formulas and runs guarded idempotent updates on every launch.

See [BUILD.md](BUILD.md) for commands.

## ⚠ Traps

1. **Stale dev database.** The app reads `%LOCALAPPDATA%/EQLBuilder/wiki.db` when it exists.
   After any data change you must copy the rebuilt `resources/wiki.db` over it and touch
   `src-tauri/src/main.rs`, or you'll debug stale data for an hour.
2. **Incremental sync clobbers enriched spell tables.** `sync --incremental` re-derives a changed
   spell's child rows from the wiki page, wiping data that came from other importers:
   `spell_source`, `spell_item_source`, `class_vendor_directory`, and `spell_pet_summon`. Back up
   first, then restore those four tables from the pre-sync backup. `spell_class_level` churn is
   legitimate wiki data — keep it.
3. **Svelte 5 keyed `{#each}` hard-throws on duplicate keys.** Key by a unique id (slot, index,
   pageid) — never by display name or a shared game label. Unkeyed `each` recycles DOM and
   causes checkbox state bugs.
4. **Export folder**: use `dirs::desktop_dir()` (it follows OneDrive redirection), never
   `USERPROFILE\Desktop`. Never open a SQLite handle inside a cloud-synced folder.
5. **Excel `ROUND` is half-away-from-zero**, matching Rust `f64::round`. Python's banker's
   rounding differs — this broke the estimator port until it was fixed.
6. **Latching UI state on build swap**: reset `editSlot` / `openSlot` in a `$effect` that tracks
   the build reference.

## Verified rules and formulas

All live in editable `formula_table` rows unless noted.

- **Caps**: buffed attribute cap **510** (`stat_cap`), save/resist cap **1000** (`resist_cap`).
  Naked starting-attribute ceiling 150 (`stat_naked_ceiling`, separate).
- **Item upgrade tiers** — VERIFIED to 100% parity with the community estimator, hardcoded in
  `eql_data::item_tier_stat/dmg/haste`: stats `0 < B ≤ 10` → `B + tier`; stats `> 10` →
  `INT(B + ROUND(B × tier) / 10)`; DAMAGE always uses the `> 10` form (9 → 13 at +5, not 14);
  negatives → `MIN(0, B + tier)`; haste +1%/tier; delay unchanged.
- **Base HP/mana** (`stats::estimator_base`): community per-class per-level curves in
  `class_base_curve`. `HP_c = INT(hp + hp_fac × adjSTA)`, `Mana_c = INT(mana + mana_fac ×
  convStat)`; top-2 classes summed, +5 flat HP. `adjSTA` halves past 255. Mana types: none for
  WAR/MNK/ROG/BER, INT for SHD/BRD/NEC/WIZ/MAG/ENC, WIS for the rest. Validated to ~2–7%
  against live screenshots.
- **Spell tiers** — community-reconstructed, NOT client-verified: damage/heal +6%/tier linear
  floor; mana −6%/tier provisional (floor 20); cast/reuse −4%/tier; reagent 10%/tier.
- **Pet slots** — VERIFIED: base 4 + sum of class bonuses (MAG +3, BST +3, NEC +2,
  ENC/DRU/SHM +1, SHD +0). Max 12 for MAG/BST/NEC.
- **Pet gear class pool** — the PET's own classes, not the owner's. A WAR/BST summon uses WAR
  gear regardless of the owner's trio. Falls back to owner classes when the summon's classes are
  unknown. Same rule gates Exaltation augments in pet gear.
- **Pet scaling**: only actually-gained levels (past the player−1 cap) grant +6% HP, +1 damage,
  +5 skill.
- **Multiclass stats**: race base + exactly 30 per class, additive. HP/mana = sum of top 2.
- **Class unlock**: slot 1 always; slot 3 at level 11 (`class_3_unlock_level`).
- **Focus limits** — client-exact, from the client's own effect data. SPA 124 damage / 125 heal /
  127 haste / 128 duration / 129 range / 132 mana / 131 reagent; limits 134 max-level (with
  decay), 138 spell-type, 140 min-duration, 141–142 min-level.

### Known incomplete

**Player ATK is not modelled.** It currently shows gear + buffs only and is flagged
`NEEDS_INGAME_TEST` in `stats.rs`. The base contribution (STR / offense / weapon skill) is most
of the real value, and EQL's formula is unpublished and roughly 3× off classic EQEmu, so no
fabricated formula was shipped. **To fix it, someone needs in-game readings** — with buffs off
and one weapon type, for several characters and levels: level, classes, STR, the in-game ATK
number, the equipped weapon's skill value, Offense skill value, and total worn item ATK. Vary
one thing at a time. Then fit `base_atk = a·offense + b·weaponskill + c·(STR − k) + d` and wire
it through `atk_*` formula rows. This is a genuinely valuable contribution and mostly needs a
patient player, not a programmer.

## Game file formats

The app reads and writes files in the user's game install. Paths below are relative to it.

- **Inventory dump** — `/outputfile inventory` produces `<Char>_<city>-Inventory.txt`,
  tab-separated. Tiers are `" +N"` name suffixes. **Game item ids ≠ wiki pageids** — bridge by
  name. Augments appear as sub-rows in slots 1/7/8/9/10.
- **Spell loadouts** — `<Char>_<city>_LO1.ini`, section `[SpellLoadouts]`. ⚠ **This file is the
  character's entire settings**, with loadouts as just one section among hot buttons, socials,
  combat, and sound. Writing loadouts back must be a surgical merge: rewrite only the edited
  sets' keys, keep every other section byte-identical, back up to `.bak` first.
- **Socials (= macros)** — the `[Socials]` section of that same file. Keys
  `Page<P>Button<B>{Name,Color,Line1..Line5}`. Standard grid is 10 pages × 12 buttons. Macro
  tokens: `%t` target, `%s`/`%o` pronouns, `/pause N` = N tenths of a second.
- **AdvLoot filter** — `userdata/LF_<Char>_<city>.ini`, format
  `ITEM_ID^FILTER_ID^ICON_ID^ITEM_NAME`. ITEM_ID is the **game item id and is tier-independent**,
  so one entry matches every tier; the `+N` in the name is cosmetic. FILTER_ID is the
  Edit-Loot-Filters column order: **1 = Loot, 2 = Merge, 3 = Store, 4 = Sell** (confirmed from an
  in-game screenshot — it is NOT Need/Greed/Never, which was an early wrong guess).

## Data pipeline

`eql_wiki_sync.py` → derived-data importers → `import_supplemental.py` → `make_dist_db.py`
(slims the mirror into `resources/wiki.db`, with row-count floors that fail the build loudly if
data went missing). Importers take env vars for local file locations rather than hard-coded
paths — keep it that way.

`make_dist_db.py`'s row-count floors are a safety net, not decoration. If you add a table the app
depends on, add a floor for it.

## Non-canonical entries

Ids `777000–777999` are reserved for deliberately non-canonical data, stored as `canonical = 0`
in SQLite and filtered out by default. They are intentional. Do not correct them against live
game data, do not remove them, and do not let an assistant "fix" them. See
[CONTRIBUTING.md](CONTRIBUTING.md).

Three things about the implementation are easy to get wrong:

1. **The Rust field is the negative — `Item.non_canonical`** — even though the SQL column is
   `canonical`. That is deliberate: `Item::default()`, a partial struct literal, and a saved
   build from before the field existed must all mean "ordinary real item". A positively-named
   `canonical: bool` defaults to `false` and silently makes every test-constructed item
   non-canonical. That bug is easy to reintroduce and the failure looks like "the optimizer
   returns nothing".
2. **Two filters are needed, in different places.** `db::load_items` filters the picker list
   (not `load_items_full`, so a saved build that already equips one still resolves instead of
   reporting an unknown item). `optimizer::legal` filters the optimizer *separately*, because
   the optimizer walks the whole snapshot rather than the picker list — without it, entries
   with deliberately absurd stats outscore every real item and get auto-equipped for everyone.
3. **Duplicate class strings are legal.** The reveal puts the same class in `build.classes`
   three times. Anything iterating `build.classes` in a keyed Svelte `{#each}` must dedupe
   first or it hits trap 3 above and takes the UI down with it.
4. **Leave `era` unset on non-canonical entries.** The default build enables only the live
   expansions, and every picker filters on it — an invented era ("Brine") is in nobody's
   enabled list and has no toggle, so the entries silently vanish from the pickers while
   the tests (which default to all-eras) stay green. `era: null` passes every era filter.

`tests/pickle_mode.rs` guards all of this in both directions — revealed and ordinary.
