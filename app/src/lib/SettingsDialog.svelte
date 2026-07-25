<script lang="ts">
  // Settings = the editable game rules (formula_table). Everything the wiki does
  // not document lives here as an assumption the player can measure and correct;
  // set_formula refreshes the engine snapshot, so `onchanged` re-runs the pipeline.
  import { listFormulas, setFormula, type FormulaRow } from "./api";

  let { onclose, onchanged }: { onclose: () => void; onchanged: () => void } = $props();

  let rows = $state<FormulaRow[]>([]);
  let drafts = $state<Record<string, string>>({});   // key -> edited value
  let measured = $state<Record<string, boolean>>({}); // key -> "I measured this in game"
  let savedKey = $state<string | null>(null);         // inline "saved" flash
  let busyKey = $state<string | null>(null);
  let err = $state<string | null>(null);
  let loading = $state(true);

  // unverified first — those are the numbers that need attention
  const RANK: Record<string, number> = {
    NEEDS_INGAME_TEST: 0,
    PARTIALLY_VERIFIED: 1,
    LEGACY_EQ_DATA: 1,
    MANUAL_OVERRIDE: 2,
    VERIFIED_INGAME: 3,
    WIKI_CONFIRMED: 3,
  };
  const rank = (st: string) => RANK[st] ?? 2;

  function badgeClass(st: string): string {
    switch (st) {
      case "WIKI_CONFIRMED":
      case "VERIFIED_INGAME": return "green";
      case "PARTIALLY_VERIFIED":
      case "LEGACY_EQ_DATA": return "yellow";
      case "NEEDS_INGAME_TEST": return "orange";
      case "MANUAL_OVERRIDE": return "blue";
      default: return "";
    }
  }

  /** Re-read the rules. `savedFor` (the key we just wrote) snaps back to the stored
   *  value; every OTHER row keeps its unsaved edit so one save can't wipe them. */
  async function refresh(savedFor?: string) {
    try {
      const list = await listFormulas();
      list.sort((a, b) =>
        rank(a.verification_status) - rank(b.verification_status) ||
        a.formula_key.localeCompare(b.formula_key)
      );
      const prev = drafts;
      const next: Record<string, string> = {};
      for (const r of list) {
        const draft = prev[r.formula_key];
        next[r.formula_key] = draft != null && r.formula_key !== savedFor ? draft : r.value;
      }
      rows = list;
      drafts = next;
      err = null;
    } catch (e) {
      err = String(e);
    }
    loading = false;
  }
  refresh();

  const dirty = (r: FormulaRow) => (drafts[r.formula_key] ?? r.value) !== r.value;

  async function save(r: FormulaRow) {
    const key = r.formula_key;
    busyKey = key;
    try {
      await setFormula(key, drafts[key] ?? r.value, measured[key] === true);
      measured[key] = false;
      await refresh(key);
      savedKey = key;
      setTimeout(() => { if (savedKey === key) savedKey = null; }, 2500);
      onchanged(); // Rust already refreshed the snapshot — force a re-resolve
    } catch (e) {
      err = String(e);
    }
    busyKey = null;
  }

  const unverified = $derived(rows.filter((r) => rank(r.verification_status) < 2).length);

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window {onkeydown} />

<!-- backdrop: click outside closes -->
<div
  class="backdrop"
  role="button"
  tabindex="-1"
  onclick={onclose}
  onkeydown={(e) => e.key === "Enter" && onclose()}
>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="dlg" role="dialog" tabindex="-1" aria-modal="true" aria-label="Settings" onclick={(e) => e.stopPropagation()}>
    <div class="titlebar">Settings</div>

    <div class="body">
      <h1>Game rules &amp; formulas</h1>
      <p class="blurb">
        These are the numbers the wiki does not document. Anything not green is an
        assumption — measure it in game and correct it here; every stat that depends on
        it updates immediately.
      </p>

      {#if err}
        <p class="err">{err}</p>
      {/if}

      {#if loading}
        <p class="muted">Loading rules…</p>
      {:else if rows.length === 0}
        <p class="muted">No editable rules found in the database.</p>
      {:else}
        <table>
          <thead>
            <tr>
              <th>Rule</th>
              <th class="vcol">Value</th>
              <th>Status</th>
              <th>Source</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {#each rows as r (r.formula_key)}
              <tr class:attention={rank(r.verification_status) < 2}>
                <td class="rule">
                  <span class="key">{r.formula_key}</span>
                  {#if r.is_user_edited}<span class="edited" title="you edited this">edited</span>{/if}
                  {#if r.description}<span class="desc">{r.description}</span>{/if}
                </td>
                <td class="vcol">
                  <input
                    class="val"
                    class:dirty={dirty(r)}
                    value={drafts[r.formula_key] ?? r.value}
                    oninput={(e) => (drafts[r.formula_key] = e.currentTarget.value)}
                  />
                </td>
                <td>
                  <span class="badge {badgeClass(r.verification_status)}">
                    {r.verification_status.replace(/_/g, " ").toLowerCase()}
                  </span>
                </td>
                <td class="src">{r.source ?? "—"}</td>
                <td class="act">
                  <label class="meas" title="promotes this rule to VERIFIED_INGAME">
                    <input type="checkbox" bind:checked={measured[r.formula_key]} />
                    I measured this in game
                  </label>
                  <button
                    class="save"
                    disabled={!dirty(r) || busyKey === r.formula_key}
                    onclick={() => save(r)}
                  >Save</button>
                  {#if savedKey === r.formula_key}
                    <span class="ok">saved — recalculating</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>

        <p class="footnote">
          {unverified} of {rows.length} rules are still assumptions. Checking
          “I measured this in game” marks the rule verified in the database.
        </p>
      {/if}
    </div>

    <div class="foot">
      <button class="ok-btn" onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed; inset: 0; background: rgba(0, 0, 0, .6);
    display: flex; align-items: center; justify-content: center; z-index: 50;
    border: 0; padding: 0;
  }
  .dlg {
    width: 900px; max-width: 94vw; max-height: 86vh;
    display: flex; flex-direction: column;
    background: linear-gradient(160deg, #0d0f14, #1a1d24);
    border: 2px solid #8a7440; border-radius: 3px;
    box-shadow: inset 0 0 0 1px #3a3f4a, 0 12px 40px rgba(0, 0, 0, .6);
    text-align: left;
  }
  .titlebar {
    text-align: center; font-variant: small-caps; letter-spacing: .2em;
    color: #c9b26a; font-size: .8rem; padding: 4px 0;
    background: linear-gradient(#181b22, #12141a); border-bottom: 1px solid #3a3f4a;
  }
  .body { padding: 1rem 1.2rem; overflow-y: auto; }
  h1 { margin: 0; font-size: 1.05rem; color: #e6e6e6; }
  .blurb { margin: .35rem 0 .9rem; color: #9ab; font-size: .78rem; line-height: 1.55; max-width: 68ch; }
  .muted { color: #667; font-size: .8rem; }
  .err { color: #f88; font-size: .8rem; }

  table { width: 100%; border-collapse: collapse; font-size: .78rem; }
  th {
    text-align: left; color: #89a; font-weight: 600; font-size: .68rem;
    text-transform: uppercase; letter-spacing: .08em;
    border-bottom: 1px solid #3a3f4a; padding: 4px 8px 4px 0;
  }
  td { padding: 6px 8px 6px 0; border-bottom: 1px solid #1e2229; vertical-align: top; }
  tr.attention td { background: rgba(200, 150, 60, .04); }

  .rule { max-width: 26ch; }
  .key { color: #e6e6e6; font-family: ui-monospace, monospace; font-size: .76rem; }
  .desc { display: block; color: #667; font-size: .7rem; line-height: 1.4; margin-top: 2px; }
  .edited {
    margin-left: .3rem; color: #c9b26a; font-size: .6rem; text-transform: uppercase;
    letter-spacing: .06em;
  }

  .vcol { width: 120px; }
  .val {
    width: 110px; padding: 3px 6px; background: #1c1f26; border: 1px solid #333;
    color: #e6e6e6; border-radius: 6px; font: inherit;
  }
  .val.dirty { border-color: #8a7440; color: #fc6; }

  .badge {
    display: inline-block; padding: 1px 7px; border-radius: 9px; font-size: .65rem;
    text-transform: uppercase; letter-spacing: .05em; white-space: nowrap;
    border: 1px solid #333; color: #9aa;
  }
  .badge.green  { background: #14301f; color: #6c9; border-color: #2a6; }
  .badge.yellow { background: #332b12; color: #dc7; border-color: #8a7440; }
  .badge.orange { background: #3a2412; color: #fa6; border-color: #a86; }
  .badge.blue   { background: #142433; color: #7bc; border-color: #35597a; }

  .src { color: #667; font-size: .7rem; max-width: 22ch; word-break: break-word; }

  .act { white-space: nowrap; }
  .meas {
    display: block; color: #89a; font-size: .68rem; margin-bottom: 4px; cursor: pointer;
  }
  .save {
    background: #22262d; color: #c9b26a; border: 1px solid #8a7440; border-radius: 6px;
    padding: 3px 12px; cursor: pointer; font: inherit; font-size: .74rem;
  }
  .save:hover:not(:disabled) { background: #2a2f38; }
  .save:disabled { opacity: .35; cursor: default; }
  .ok { color: #6c9; font-size: .7rem; margin-left: .4rem; }

  .footnote { color: #778; font-size: .7rem; margin: .7rem 0 0; }

  .foot {
    padding: .5rem .8rem; border-top: 1px solid #262b33; text-align: right;
    background: #12141a;
  }
  .ok-btn {
    background: #22262d; color: #c9b26a; border: 1px solid #8a7440;
    border-radius: 6px; padding: 4px 16px; cursor: pointer; font: inherit;
  }
  .ok-btn:hover { background: #2a2f38; }
</style>
