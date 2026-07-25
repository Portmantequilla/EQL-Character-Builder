<script lang="ts">
  // Skill lines for the build's classes, merged BEST_OF (plan §4.7 formula
  // multi_class_skill_combine): the highest cap across classes wins, the earliest
  // training level is shown. Data: eqlbuilds.com snapshot (client-extracted;
  // PARTIALLY_VERIFIED — server can override).
  import type { AppState } from "../state.svelte";
  import { querySkills, type SkillRow } from "../api";

  let { s }: { s: AppState } = $props();

  let skills = $state<SkillRow[]>([]);
  let loading = $state(false);
  let search = $state("");

  let gen = 0;
  $effect(() => {
    const classes = [...s.build.classes];
    const g = ++gen;
    if (classes.length === 0) { skills = []; loading = false; return; }
    loading = true;
    querySkills(classes)
      .then((r) => { if (g === gen) { skills = r; loading = false; } })
      .catch((e) => { if (g === gen) { loading = false; s.error = String(e); } });
  });

  const shown = $derived(
    skills
      .filter((sk) => !search || sk.name.toLowerCase().includes(search.toLowerCase()))
      .sort((a, b) => (a.trained_at ?? 999) - (b.trained_at ?? 999) || a.name.localeCompare(b.name))
  );
  const trainableNow = $derived(shown.filter((sk) => (sk.trained_at ?? 999) <= s.build.level));
</script>

<div class="filters">
  <input class="search" placeholder="search skills…" bind:value={search} />
  <span class="count">
    {shown.length} skill lines · {trainableNow.length} trainable at level {s.build.level}{loading ? " · loading…" : ""}
  </span>
</div>

<p class="legend">
  Multiclass combine: <strong>best cap wins</strong> across {s.build.classes.join(" / ") || "your classes"}
  (plan rule BEST_OF, NEEDS_INGAME_TEST) · caps are the class maximums from the
  eqlbuilds.com client snapshot — the live server can differ.
</p>

{#if skills.length === 0 && !loading}
  <p class="muted">No skill data — pick classes, or run scripts/import_eqlbuilds.py.</p>
{:else}
  <table>
    <thead>
      <tr>
        <th>Skill</th><th class="num">Trained at</th><th class="num">Cap</th>
        <th>Best from</th><th>Classes</th>
      </tr>
    </thead>
    <tbody>
      {#each shown as sk (sk.name)}
        <tr class:locked={(sk.trained_at ?? 999) > s.build.level}>
          <td>{sk.name}</td>
          <td class="num">{sk.trained_at ?? "—"}</td>
          <td class="num cap">{sk.cap ?? "—"}</td>
          <td class="best">{sk.best_class ?? "—"}</td>
          <td class="cls">{sk.classes.join(" ")}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  .filters { display: flex; gap: .6rem; align-items: center; flex-wrap: wrap; margin-bottom: .4rem; }
  .search { min-width: 220px; padding: 5px 8px; background: #1c1f26; border: 1px solid #333; color: #e6e6e6; border-radius: 6px; }
  .count { color: #667; font-size: .8rem; }
  .legend { color: #987; font-size: .74rem; margin: 0 0 .5rem; }
  .legend strong { color: #c9b26a; }
  .muted { color: #667; }
  table { border-collapse: collapse; font-size: .84rem; min-width: 560px; }
  th { text-align: left; color: #89a; font-weight: 600; padding: 3px 14px 3px 0; border-bottom: 1px solid #333; position: sticky; top: 0; background: #14161a; }
  th.num, td.num { text-align: right; }
  td { padding: 2px 14px 2px 0; border-bottom: 1px solid #20242b; }
  td.cap { color: #fc6; font-weight: 600; }
  td.best { color: #c9b26a; }
  td.cls { color: #9ab; font-size: .78rem; }
  tr.locked td { opacity: .45; }
</style>
