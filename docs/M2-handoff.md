# M2 handoff — port the reference resolver to the Rust engine

**For: Claude Code, running on this machine (can compile Rust; Cowork could not).**
Canonical plan: `docs/character-builder-plan.md` (§4.1 engine placement, §4.4 the 12-step
resolver, §4.9 test plan, risk 8 "same test corpus validates a port"). This doc is the
concrete first slice.

## What already exists (do not rebuild)

- **Data**: `db/eql.db` fully synced (items 10,758 · mobs 6,340 · spells 1,887 + class
  levels/effects/stacking). Buff-line tables now populated by
  `scripts/import_buff_lines.py`: `buff_line` (117: 112 wiki + 5 pet-seed),
  `buff_line_member` (470, 99.4% linked, 45 bard dual-values), `spell_buff_line`.
  Re-run offline any time: `python scripts/import_buff_lines.py --db db/eql.db`.
  Inspectable export: `exports/buff_lines.json`.
- **Reference resolver (the spec)**: `scripts/resolve_buffs.py` — engine step 9, pure and
  deterministic. `resolve(build, level, target, bard_in_group, lines, clevels, names)`
  returns, per line, the strongest *available* member classified SELF_CAST (a build class
  learns the spell by `level`) / ITEM (click/proc/worn/consumable) / EXTERNAL / UNKNOWN.
- **Golden oracle**: `scripts/test_resolve_buffs.py` — 5 checks, all passing. This is your
  acceptance corpus.
- **App scaffold**: `app/` (Tauri v2 + Svelte 5 + Rust workspace). `crates/eql-engine`
  already has `item_wearable_by` + a `calculate` stub and unit tests. `crates/eql-data`
  holds shared types. `src-tauri` reads SQLite and exposes `query_items`/`get_snapshot`.

## The M2 task (first slice)

Implement buff-line resolution (step 9) in `crates/eql-engine` so it reproduces the
reference resolver, then grow toward the full §4.4 resolver.

1. In `eql-data`, add the buff types (`BuffLine`, `BuffLineMember` with `source_kind`,
   `value_base`, `value_max_instrument`, `is_group`, `is_self_only`, `spell_id`,
   `member_name_raw`) and a `MemberStatus` enum {SelfCast, Item, External, Unknown}.
2. In `eql-engine`, add `resolve_buff_lines(snapshot, build, level, target, bard_in_group)`
   mirroring `resolve_buffs.py::availability` + strongest-per-line selection. **Keep the
   crate I/O-free** — the caller (src-tauri) loads `buff_line`/`buff_line_member` and the
   spell class-level map from SQLite and passes them in the `Snapshot`.
3. In `src-tauri`, load those rows (join `spell_class_level` for castability) and add a
   `resolve_buffs(classes, level, target)` Tauri command.
4. Port the 5 golden cases from `test_resolve_buffs.py` into `#[cfg(test)]` in
   `eql-engine` (Maniacal Strength +68 self-cast; Focus of Spirit +67; Strength(Anthem)
   unfillable without bard; level gating L56→Strength +67, L57→Maniacal +68).

## Acceptance criteria (M2 first slice DoD)

- `cargo test -p eql-engine` green, including the 5 ported golden cases.
- For SHD/MNK/SHM @L60, engine reports **70 fillable player lines** (20 self-cast, 50 item)
  — identical to `python scripts/resolve_buffs.py SHD MNK SHM`.
- Engine crate has zero `rusqlite`/`tauri` imports (grep to confirm). Determinism: same
  inputs → byte-identical output.
- `cargo tauri dev` launches; the window can call `resolve_buffs` and render the plan.

## Known refinements to carry forward (not required for the first slice)

- **bard-in-group**: currently only scales instrument values; should also make
  BRD-castable lines *available* when a bard is present (cast by the group bard).
- **pet lines**: values are NULL (NEEDS_INGAME_TEST); resolver returns "unfillable" —
  keep that honesty; fill via `overrides/seeds/pet_buff_lines.yaml` when measured.
- **not yet modeled**: multi-stat combination consumption (one spell occupying >1 line —
  `combination_group_id`/`spell_buff_line` already carry the data), the 15-buff cap
  including bard songs, worn/focus item mods folded in, and the full §4.4 12-step order.

## Environment gotchas (bit us in Cowork)

- **Never open a DB under OneDrive** (plan risk 4). SQLite on the OneDrive path throws
  `disk I/O error`. Build to `%LOCALAPPDATA%/EQLBuilder` or `%TEMP%` and copy back
  (`set EQL_DB=%TEMP%\eql.db`). The app already targets `%LOCALAPPDATA%`.
- The `db/eql.db` you have locally is the real one; a copy seen elsewhere may be a stale
  OneDrive placeholder. Trust the freshly-synced local file.
