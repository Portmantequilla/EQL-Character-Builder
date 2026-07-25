<script lang="ts">
  // What an effect actually DOES: the linked spell's parsed wiki lines ("Increase
  // Attack Speed by 64%"), duration/target/mana. Shared by augment cards, socket rows,
  // and the Stats page effect list. Honest fallback when the spell link is missing.
  import { spellDetails, type SpellDetails } from "./api";

  let { spellId, effectName }: {
    spellId: number | null;
    effectName: string; // shown while loading / when no spell is linked
  } = $props();

  let det = $state<SpellDetails | null>(null);
  let missing = $state(false);
  $effect(() => {
    det = null;
    missing = false;
    if (spellId == null) {
      missing = true;
      return;
    }
    const id = spellId;
    getDetails(id).then((d) => {
      if (spellId !== id) return; // prop changed while fetching
      if (d) det = d;
      else missing = true;
    });
  });
</script>

<script lang="ts" module>
  import { spellDetails as fetchDetails, type SpellDetails as SD } from "./api";
  // per-run cache: effect spells are static wiki data
  const cache = new Map<number, SD | null>();
  const pending = new Map<number, Promise<SD | null>>();
  function getDetails(id: number): Promise<SD | null> {
    if (cache.has(id)) return Promise.resolve(cache.get(id) ?? null);
    let p = pending.get(id);
    if (!p) {
      p = fetchDetails([id])
        .then((m) => {
          const d = m[id] ?? null;
          cache.set(id, d);
          return d;
        })
        .catch(() => null)
        .finally(() => pending.delete(id));
      pending.set(id, p);
    }
    return p;
  }
</script>

<div class="explain">
  {#if det}
    {#each det.effects as line, i (i)}
      <div class="line">{line}</div>
    {/each}
    {#if det.effects.length === 0}
      <div class="line dim">no parsed effect lines for {det.name}</div>
    {/if}
    <div class="meta">
      {#if det.target_type}<span>Target: {det.target_type}</span>{/if}
      {#if det.duration}<span>Duration: {det.duration}</span>{/if}
      {#if det.mana != null}<span>Mana: {det.mana}</span>{/if}
    </div>
  {:else if missing}
    <div class="line dim">
      {effectName}: effect details not in the wiki data yet (no linked spell)
    </div>
  {:else}
    <div class="line dim">loading effect details…</div>
  {/if}
</div>

<style>
  .explain { font-size: .72rem; color: #9cd; }
  .line { color: #9cd; }
  .line.dim { color: #667; font-style: italic; }
  .meta { display: flex; gap: .7rem; color: #778; margin-top: 2px; flex-wrap: wrap; }
</style>
