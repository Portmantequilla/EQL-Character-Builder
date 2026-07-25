# EQL Character Builder

**A free, offline character builder for EverQuest Legends.**
Too many tabs to count, the game's own numbers, and not a single connection required.

[**Download**](https://beecanyonretro.com/#download) ·
[Report a wrong number](../../issues/new/choose) ·
[Build from source](BUILD.md) ·
[Contribute](CONTRIBUTING.md)

> ⚠️ **Early days — a work in progress.** Not all of the game's data was available when this was
> made, so some numbers are incomplete or approximate. It gets more accurate **with the
> community's help** — if you spot something wrong, [please tell me](../../issues/new/choose).
> You don't need a GitHub account or any code to help; see [CONTRIBUTING.md](CONTRIBUTING.md).

Plan your whole character before you commit a point in-game — gear and upgrade tiers, spells,
buffs, pets, loot filters, macros, and the stat math behind them. Runs entirely on your machine.
Free, offline, no account, nothing phones home.

## What it does

| Area | What you get |
|---|---|
| **Gear & upgrade tiers** | Every worn slot, item stats and tier upgrades, an Optimal / Min-Max optimizer, bulk tier slider. |
| **Spell Manager** | Your spellbook with client-exact damage / heal values, precise focus limits, upgrade tiers. |
| **Buffs & stacking** | Self, group, and external buffs — what stacks, what's over-cap, real totals. |
| **Pet builder** | Full pet paperdoll, per-slot gear, class-correct slot counts, survival-gear suggester. |
| **Loot Filter forge** | Edit the game's AdvLoot `LF_*.ini` filters without touching a text editor. |
| **Macro library** | Build and manage in-game socials/macros, with live command validation. |
| **Stats engine** | HP, mana, attributes, resists — with caps and over-cap flagged. |
| **AA, skills & more** | Alternate advancement, skills, stances, and the rest. |

## For developers

Tauri v2 + Svelte 5 + a Rust workspace. The interesting part is the engine:

```
app/
├─ crates/eql-engine/   pure calculation — no I/O, no Tauri. Deterministic, golden-tested.
├─ crates/eql-data/     shared types + pure helpers (tier rules, slot maps)
├─ src-tauri/           SQLite + Tauri commands — the only I/O layer
└─ src/                 Svelte UI (tabs in src/lib/tabs/)
scripts/                Python data pipeline (wiki sync + importers)
overrides/seeds/        hand-maintained seed data
```

- [BUILD.md](BUILD.md) — prerequisites, running it, building the database
- [AGENTS.md](AGENTS.md) — architecture, verified formulas, and the traps. **Read this before
  touching the engine.** Also the context file to point your AI assistant at.
- [CONTRIBUTING.md](CONTRIBUTING.md) — DCO sign-off, review times, how to help without code
- [DATA_SOURCES.md](DATA_SOURCES.md) — full credits

```bash
cd app && npm install && npm run tauri dev
cargo test && npm run check
```

## License

[AGPL-3.0](LICENSE) — free software, and it stays that way. Fork it, change it, share it; if you
host a modified version, publish your changes too. See [NOTICE.md](NOTICE.md) for third-party
game content, trademarks, and the no-affiliation statement.

Not affiliated with or endorsed by the EverQuest Legends team.

---

**Bee Canyon Retro** · [beecanyonretro.com](https://beecanyonretro.com)
