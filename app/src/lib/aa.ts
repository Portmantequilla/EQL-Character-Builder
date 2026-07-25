// Alternate Advancement helpers shared by the AA tab and the Spellbook tab.
// The mutation helpers write `build.aa_ranks` — the ONE source of truth for AA ranks.
import type { AaAbility, BuildInput } from "./api";

/** The AA that grants spell gems (8 base + 1 per rank, 6 ranks -> 14 gems). */
export const MNEMONIC_AA = "Mnemonic Retention";

/** Category sections, in display order. */
export const AA_CATEGORIES = ["GENERAL", "ARCHETYPE", "CLASS", "SPECIAL"] as const;

export function findAaByName(aas: AaAbility[], name: string): AaAbility | null {
  return aas.find((a) => a.name === name) ?? null;
}

/** Purchased rank of one AA (absent key = 0). */
export function aaRank(build: BuildInput, id: number): number {
  return build.aa_ranks?.[id] ?? 0;
}

/**
 * Set a rank, clamped to 0..max_rank. Rank 0 DELETES the key (a stored zero would
 * be noise in the saved build) and every write hands back a fresh object so the
 * resolve pipeline sees the change.
 */
export function setAaRank(build: BuildInput, aa: AaAbility, rank: number): void {
  const clamped = Math.min(aa.max_rank, Math.max(0, Math.floor(rank)));
  const next = { ...(build.aa_ranks ?? {}) };
  if (clamped === 0) delete next[aa.id];
  else next[aa.id] = clamped;
  build.aa_ranks = next;
  // The engine reads max(planner, legacy field), so a legacy value would fight a
  // lowered stepper. Once the planner owns Mnemonic Retention, retire the old field.
  if (aa.name === MNEMONIC_AA) build.aa_mnemonic_retention = 0;
}

/**
 * Cost of holding `rank` ranks: the wiki's per-rank costs are CUMULATIVE, so this is
 * the sum of the first N entries. Mirrors eql_engine::aa::rank_cost — a "?" rank is
 * unknowable, never free, so it is counted separately and NEVER folded in as 0.
 */
export function aaCost(aa: AaAbility, rank: number): { points: number; unknown: number } {
  let points = 0;
  let unknown = 0;
  for (let i = 0; i < Math.min(rank, aa.max_rank); i++) {
    const c = aa.costs[i];
    if (c == null) unknown++;
    else points += c;
  }
  return { points, unknown };
}

/** "12" when every rank is known, "4+?" / "?" when the wiki left ranks blank. */
export function fmtAaCost(aa: AaAbility, rank: number): string {
  const { points, unknown } = aaCost(aa, rank);
  if (unknown === 0) return String(points);
  return points === 0 ? "?" : `${points}+?`;
}

/** CLASS AAs are gated by the build's classes; the other categories are offered to all. */
export function aaGrantedByClasses(aa: AaAbility, classes: string[]): boolean {
  if (aa.category !== "CLASS") return true;
  const abbr = aa.class_abbr?.toUpperCase() ?? null;
  return abbr != null && classes.some((c) => c.toUpperCase() === abbr);
}
