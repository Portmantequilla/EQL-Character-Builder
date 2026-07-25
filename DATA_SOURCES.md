# Data sources & credits

This project is assembled from the EverQuest Legends community's work. None of the game data is
mine, and this repository does not redistribute it — the pipeline builds a local database from
sources you already have access to. See [NOTICE.md](NOTICE.md).

## Sources

### [eqlwiki.com](https://eqlwiki.com)
The item, mob, and spell reference — the stat source of record. Mirrored by
`scripts/eql_wiki_sync.py`. Content belongs to that site's contributors and remains subject to
its terms.

### [eqlbuilds.com](https://eqlbuilds.com)
Stances, invocations, class skills, client damage/heal ranges, and AA cost data, accessed via
the MIT-licensed [everquest-legends-mcp](https://github.com/ArtSabintsev/everquest-legends-mcp)
by Arthur Sabintsev.

### [eqltools.com](https://eqltools.com)
Starting attributes, the multiclass stat-combine rule, and the stat/resist caps.

### Mosscovered Legend's EQL Stat Estimator
HP and mana curves per class and level, and the exact item upgrade-tier formulas. By **kosstile**
with contributors **cactot, Dannuic, Gigglemage, morbes, Tubasnot,** and **Walker**. The item
tier rule is reproduced exactly (verified to 100% parity); the HP/mana curves are imported as
data. This is the single largest external contribution to the accuracy of this tool.

### The EverQuest Legends client
Spell damage, heal values, and focus limits are read from the game's own data files on the
user's machine — the same files the game reads. This yields authoritative values for spells the
community sources don't cover. **These files are not redistributed here**; the importer reads
the copy in your own installation.

### The EQL community
Caps, mechanics, and correction reports from players, including via the community Discord.
Every verified correction improves the tool for everyone.

## Accuracy and honesty

Where a value is an estimate, unverified, or community-reconstructed, the app says so rather
than presenting it as fact. Several models — player ATK most notably — are explicitly incomplete
and flagged in the interface. See [CONTRIBUTING.md](CONTRIBUTING.md) if you can help fill a gap.

A small number of entries are **deliberately non-canonical** and hidden by default; see the
pickle wizard section of CONTRIBUTING.md.

## Corrections and removal requests

If you maintain one of these sources and want attribution changed, or want this project to stop
using your data, open an issue and I'll act on it.
