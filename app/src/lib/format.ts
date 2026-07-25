// Small pure helpers shared by the tabs.
import type { Item, MemberStatus } from "./api";

/** URL of a 40x40 item icon PNG served by vite from app/public/icons/. */
export const iconUrl = (iconId: number) => `/icons/item_${iconId}.png`;

/** URL of a spell-gem icon PNG (token from spell_icons) served from app/public/icons/. */
export const spellIconUrl = (token: string) => `/icons/spellicon_${token}.png`;

/** onerror handler for icon <img>: hide the broken image, leaving the bare well. */
export function hideOnError(e: Event) {
  const img = e.currentTarget as HTMLImageElement | null;
  if (img) img.style.display = "none";
}

/** EAR1/EAR2 -> EAR, WRIST1/2 -> WRIST, FINGER1/2 -> FINGER, ANY1/ANY2 -> ANY; else as-is. */
export function canonicalSlot(paperdollSlot: string): string {
  const m = /^(EAR|WRIST|FINGER|ANY)[12]$/.exec(paperdollSlot.toUpperCase());
  return m ? m[1] : paperdollSlot.toUpperCase();
}

/**
 * The in-game inventory window arrangement (Equipment Layout reference, 2026-07-20):
 * four stone bars of slots. Mirrors eql_data::PAPERDOLL_ROWS; used by the player
 * Equipment tab and (with a PET_ prefix) the pet paperdoll. Row order is also the
 * pet's active-slot priority order.
 */
export const PAPERDOLL_ROWS: readonly (readonly string[])[] = [
  ["EAR1", "NECK", "FACE", "HEAD", "EAR2"],
  ["FINGER1", "WRIST1", "ARMS", "HANDS", "WRIST2", "FINGER2"],
  ["SHOULDERS", "CHEST", "BACK", "WAIST", "LEGS", "FEET"],
  ["PRIMARY", "SECONDARY", "RANGE", "AMMO", "ANY1", "ANY2"],
];

/** Mirror of the engine's era rule: empty set = all; untagged items always allowed. */
export function eraAllowed(itemEra: string | null, enabled: string[]): boolean {
  if (itemEra == null || enabled.length === 0) return true;
  return enabled.includes(itemEra);
}

/**
 * EXACT community item-upgrade rule (Mosscovered Legend's EQL Stat Estimator,
 * reported 100% game parity; confirmed vs the live Keg Mallet +5 window).
 * STATS: 0 unchanged; 0<B<=10 -> B+tier; B>10 -> INT(B+ROUND(B*tier)/10);
 * B<0 -> MIN(0, B+tier). DAMAGE: always the >10 formula (9 -> 13 at +5, not 14).
 * HASTE: +1%/tier. DELAY NEVER CHANGES.
 */
export function itemTierStat(base: number, tier: number): number {
  const t = Math.min(10, Math.max(0, Math.round(tier)));
  if (t === 0 || base === 0) return base;
  if (base < 0) return Math.min(0, base + t);
  if (base <= 10) return base + t;
  return Math.floor(base + Math.round(base * t) / 10);
}
export function itemTierDmg(base: number, tier: number): number {
  const t = Math.min(10, Math.max(0, Math.round(tier)));
  if (t === 0 || base <= 0) return base;
  return Math.floor(base + Math.round(base * t) / 10);
}
export function itemTierHaste(base: number, tier: number): number {
  return base > 0 ? base + Math.min(10, Math.max(0, Math.round(tier))) : base;
}
/** LEGACY approximate bonus (kept for any remaining spell-side mirror math). */
export function tierBonus(base: number, tier: number, pct = 10): number {
  if (base <= 0 || tier === 0) return 0;
  return Math.max(Math.floor(base * (pct / 100) * tier), tier);
}

// ---- spell tier scaling (mirrors eql_data helpers; community-reconstructed 2026-07) ----
/** Damage/healing at tier: floor(base * (1 + pct/100 * T)) — linear, not compounded. */
export function spellTierValue(base: number, tier: number, pct: number): number {
  if (tier === 0) return base;
  return Math.floor(base * (1 + (pct / 100) * tier));
}
/** Mana at tier: round(base * (1 - pct/100 * T)), never negative. PROVISIONAL —
 *  callers must skip bases below the spell_tier_mana_floor formula. */
export function spellTierMana(base: number, tier: number, pct: number): number {
  if (tier === 0) return base;
  return Math.max(0, Math.round(base * (1 - (pct / 100) * tier)));
}
/** Cast/recovery/reuse seconds at tier: base * (1 - pct/100 * T), floored at 0. */
export function spellTierTime(base: number, tier: number, pct: number): number {
  if (tier === 0) return base;
  return Math.max(0, base * (1 - (pct / 100) * tier));
}

/** Ranking score for the gear picker: ac + sum of stats. */
export function itemScore(i: Item): number {
  let sum = i.ac ?? 0;
  for (const v of Object.values(i.stats)) sum += v;
  return sum;
}

// ---- gear categories (the pet picker's Weapon / Armor / Accessory buttons) ----
export type GearCategory = "weapon" | "armor" | "accessory";
const HAND_SLOTS = new Set(["PRIMARY", "SECONDARY", "RANGE", "AMMO"]);
const ARMOR_SLOTS = new Set([
  "HEAD", "FACE", "CHEST", "ARMS", "SHOULDERS", "BACK",
  "WRIST", "HANDS", "LEGS", "FEET", "WAIST",
]);

/** Weapon = deals damage / has a weapon skill / fits a hand slot (incl. shields and
 *  ammo); Armor = fits a body slot; Accessory = the rest (jewelry, clickies). */
export function gearCategory(i: Item): GearCategory {
  if (i.dmg != null || i.weapon_skill != null) return "weapon";
  const slots = (i.slots.length > 0 ? i.slots : i.slot ? [i.slot] : []).map((s) =>
    s.toUpperCase()
  );
  if (slots.some((s) => HAND_SLOTS.has(s))) return "weapon";
  if (slots.some((s) => ARMOR_SLOTS.has(s))) return "armor";
  return "accessory";
}

/** Compact one-line stat summary for an item row. */
export function itemSummary(i: Item, maxStats = 4): string {
  const parts: string[] = [];
  if (i.ac != null) parts.push(`AC ${i.ac}`);
  if (i.dmg != null) parts.push(`${i.dmg}dmg${i.atk_delay != null ? `/${i.atk_delay}dly` : ""}`);
  if (i.haste_pct != null) parts.push(`haste ${i.haste_pct}%`);
  const stats = Object.entries(i.stats)
    .sort((a, b) => Math.abs(b[1]) - Math.abs(a[1]))
    .slice(0, maxStats)
    .map(([k, v]) => `${k}${v >= 0 ? "+" : ""}${v}`);
  parts.push(...stats);
  if (i.worn_effect) parts.push(`worn: ${i.worn_effect}`);
  if (i.focus_effect) parts.push(`focus: ${i.focus_effect}`);
  return parts.join(" · ");
}

/** CSS class for a buff member status (colors defined :global in App.svelte). */
export function statusClass(s: MemberStatus): string {
  switch (s) {
    case "SELF_CAST": return "st-self";
    case "GROUP_BARD": return "st-bard";
    case "ITEM": return "st-item";
    case "EXTERNAL_CAST": return "st-extcast";
    case "EXTERNAL": return "st-ext";
    default: return "st-unk";
  }
}

const STAT_ORDER = [
  "STR", "STA", "AGI", "DEX", "WIS", "INT", "CHA",
  "AC", "HP", "MANA", "ATK", "HP REGEN", "MANA REGEN",
];

/** Plan §5 row order: attributes, AC/HP/MANA/ATK, regens, then SV *, then the rest. */
export function orderStatKeys(keys: string[]): string[] {
  const norm = (k: string) => k.toUpperCase().replace(/_/g, " ");
  const rank = (k: string) => {
    const n = norm(k);
    const i = STAT_ORDER.indexOf(n);
    if (i >= 0) return i;
    if (n.startsWith("SV")) return 100;
    return 200;
  };
  return [...keys].sort((a, b) => rank(a) - rank(b) || norm(a).localeCompare(norm(b)));
}
