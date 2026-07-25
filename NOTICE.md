# NOTICE

EQL Character Builder
Copyright © 2026 Bee Canyon Retro

This program is free software: you can redistribute it and/or modify it under the terms of the
**GNU Affero General Public License, version 3** as published by the Free Software Foundation.
See [LICENSE](LICENSE) for the full text.

This program is distributed in the hope that it will be useful, but **WITHOUT ANY WARRANTY**;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

---

## Third-party content — not covered by the above copyright

The AGPL covers **the code in this repository**. It does not, and cannot, cover material that
belongs to someone else:

- **Game data** — item and spell names, statistics, icons, and artwork originate with the
  EverQuest Legends game and its publisher and remain the property of their respective owners.
  This repository does **not** redistribute the game's data files. The build pipeline reads data
  from sources the user already has access to (their own game install and public community
  sites) and produces a local database on the user's machine.
- **Game icons** — the icon set is not included in this repository. It is fetched from public
  community sources at setup time (see [BUILD.md](BUILD.md)) or supplied as a convenience
  download alongside a release. Those images belong to their respective owners.
- **Community wiki data** — mirrored content originates with the contributors of eqlwiki.com and
  remains subject to that site's terms.

These materials are used solely to help players of the game plan characters. No ownership,
endorsement, or affiliation is claimed or implied. Full attribution is in
[DATA_SOURCES.md](DATA_SOURCES.md).

## No affiliation

This is an independent, fan-made tool. It is **not** affiliated with, endorsed by, or sponsored
by Daybreak Game Company LLC, Darkpaw Games, or the EverQuest Legends development team.
"EverQuest" and "EverQuest Legends" are trademarks of their respective owners.

## Accuracy

Game values are community-maintained and may be incorrect, incomplete, or out of date. Several
formulas in this software are explicitly unverified and are marked as such in the interface.
Some entries are **deliberately non-canonical** (see [CONTRIBUTING.md](CONTRIBUTING.md)).
Always verify against the live game.

## Open-source components

Built with Tauri, Rust, Svelte, and SQLite, each distributed under its own license
(MIT, Apache-2.0, or public domain). Those licenses govern those components and are
unaffected by this notice.
