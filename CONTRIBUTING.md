# Contributing

Help is genuinely wanted — especially **data verification**, which is where this project is
weakest. You do not need to be a programmer to make the biggest difference here.

## The fastest way to help (no GitHub account needed)

The numbers are incomplete and some are outright wrong. If you know a value from the live game:

- Open a [Needed update / correction](../../issues/new/choose) issue, **or**
- Post it in the community channels linked from [beecanyonretro.com](https://beecanyonretro.com)

Include the tab, the value shown, the correct value, and a screenshot if you have one. I convert
these into commits myself and credit you in the commit message. Most people who know these
numbers cold don't use GitHub, and I'd rather have the data than the pull request.

## Response time

**I aim to respond to issues and review pull requests within 3 days.** If I've gone quiet longer
than that, ping the thread — you're not being ignored, I've lost track.

## Recognition

Data verification counts as much as code here. Contributors are credited in the README and in
the app's own credits, and data contributions carry per-entry attribution where the schema
allows it — your handle attached to the thing people actually use.

If you keep contributing in one area, I'm happy to hand over ownership of it: **contributor →
trusted reviewer for a data domain → commit access**. Ask, or I'll offer.

## Developer Certificate of Origin (DCO)

This project uses the [DCO](https://developercertificate.org/) instead of a CLA. There is
nothing to sign. You just certify that you wrote the contribution, or otherwise have the right
to submit it under the project's license, by adding a `Signed-off-by` line to each commit:

```
git commit -s -m "Fix Blackburrow drop rates"
```

That appends:

```
Signed-off-by: Your Name <your.email@example.com>
```

Use your real name (a consistent pseudonym you're known by is fine). A CI check enforces this.
If you forget, `git commit --amend -s` and force-push the branch.

**Why DCO and not a CLA:** a CLA would let me relicense your work later, including making it
proprietary. I don't want that power and you shouldn't have to grant it. The trade-off is real
and deliberate — it means this project can never be dual-licensed commercially without asking
every contributor.

## Why AGPL-3.0

So that if someone forks this and hosts it, they have to publish their changes too. It stays
free for everyone — including for whoever wants to keep it alive if I get bored.

## Working on the code

See [BUILD.md](BUILD.md) for prerequisites and how to run it, and [AGENTS.md](AGENTS.md) for
the architecture, the verified formulas, and the traps that will otherwise cost you an evening.
**Read AGENTS.md before changing anything in `crates/eql-engine`** — the math there is
community-calibrated and several rules are non-obvious.

Ground rules:

- `cargo test -p eql-engine -p eql-data --lib` must stay green — that's what CI runs, and it
  needs nothing but the source. (A bare `cargo test` additionally runs integration tests that
  require a locally built game database; see [BUILD.md](BUILD.md).) The engine is deterministic
  and golden-tested on purpose.
- `npm run check` must report 0 errors.
- Keep the engine pure — no I/O, no Tauri, in `crates/eql-engine`.
- If a formula is unverified, label it as unverified in the UI. Do not ship a confident number
  you cannot support. Honesty about uncertainty is a feature of this project.

### Using an AI assistant

That's fine, and [AGENTS.md](AGENTS.md) exists partly to give your assistant the context it
needs to be correct rather than plausible. Two conditions:

1. **You** are responsible for what you submit. Read it, run the tests, understand it well
   enough to defend it in review. Your sign-off means you stand behind it.
2. Don't let it "fix" the non-canonical entries (below) or invent game values. Made-up numbers
   presented as real are the one thing that would genuinely damage this project.

## The pickle wizard

Some entries in the data set are **deliberately non-canonical**. They live in the reserved id
range `777000–777999`, are flagged `canonical = 0`, and are hidden from the pickers unless you
find the way in. They are intentional. They are not bugs, not corrupted data, and not a mistake
in the importer.

**Please do not "correct" them against live game data, and do not remove them.**

They exist for three reasons: they're funny, they exercise the real stat pipeline end-to-end
with an absurd input, and if this data set ever turns up somewhere it shouldn't, they make it
easy to recognise.

**PRs adding more of them are welcome** — as long as they follow the existing schema in
`overrides/seeds/supplemental_items.json`, keep to the reserved id range, and produce values the
engine can actually compute. If you want to make a first contribution and don't feel ready to
touch real balance data, this is the place to start. Make it good.

Finding the unlock is left as an exercise. It involves committing very hard to a single class.
