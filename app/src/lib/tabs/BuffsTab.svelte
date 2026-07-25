<script lang="ts">
  import type { AppState } from "../state.svelte";
  import LinesTable from "../LinesTable.svelte";
  import { listModes, listOtherBuffs, setFormula, type Mode, type OtherBuffRow } from "../api";
  import { statusClass } from "../format";

  let { s }: { s: AppState } = $props();
  const plan = $derived(s.result?.buff_plan ?? null);

  // ---- toolbar state ----
  let showAllLines = $state(false); // Show Active Only (default) vs Show All Available
  let addClassOpen = $state(false);
  let addOtherOpen = $state(false);
  let otherCatalog = $state<OtherBuffRow[] | null>(null);
  let otherSearch = $state("");

  /** Restore the recommended automatic loadout: every manual/off choice goes (spec §4). */
  function restoreDefaults() {
    s.build.disabled_buffs = [];
    s.build.disabled_lines = [];
    s.build.manual_buffs = [];
    s.build.other_buffs = [];
    s.build.external_buffs = [];
  }
  /** Empty the plan: turn OFF every line that currently has an active buff. Unlike
   *  disabling a member name, a disabled LINE gets no next-best refill. */
  function clearAll() {
    const lines = new Set(s.build.disabled_lines ?? []);
    for (const l of plan?.lines ?? []) if (l.chosen) lines.add(l.line);
    s.build.disabled_lines = [...lines];
  }
  /** the per-buff eye: turn the buff's LINE(s) off entirely (no weaker refill). */
  function toggleBuffOff(activeLines: string[]) {
    const off = new Set(s.build.disabled_lines ?? []);
    for (const l of activeLines) off.add(l);
    s.build.disabled_lines = [...off];
  }
  function reEnableLine(line: string) {
    s.build.disabled_lines = (s.build.disabled_lines ?? []).filter((l) => l !== line);
  }
  const disabledLines = $derived([...(s.build.disabled_lines ?? [])].sort());

  // ---- Add Class Buff: usable line members (chosen + alternatives) not currently
  // the pick — choosing one pins it as the line's MANUAL selection ----
  const classChoices = $derived.by(() => {
    const out: { line: string; name: string; value: number | null; spell_id: number; current: boolean }[] = [];
    for (const l of s.result?.buff_plan.lines ?? []) {
      const all = [...(l.chosen ? [l.chosen] : []), ...l.alternatives];
      for (const m of all) {
        if (m.spell_id == null) continue;
        if (m.status !== "SELF_CAST" && m.status !== "GROUP_BARD") continue;
        out.push({
          line: l.line, name: m.name, value: m.value, spell_id: m.spell_id,
          current: l.chosen?.spell_id === m.spell_id,
        });
      }
    }
    return out.sort((a, b) => a.line.localeCompare(b.line) || (b.value ?? 0) - (a.value ?? 0));
  });
  function pickManual(lineName: string, spellId: number) {
    // one manual pick per line: drop earlier picks that belong to the same line.
    // clicking the already-pinned member UNPINS it (back to the auto strongest).
    const lineIds = new Set(
      classChoices.filter((c) => c.line === lineName).map((c) => c.spell_id)
    );
    const wasPinned = (s.build.manual_buffs ?? []).includes(spellId);
    const kept = (s.build.manual_buffs ?? []).filter((id) => !lineIds.has(id));
    s.build.manual_buffs = wasPinned ? kept : [...kept, spellId];
  }
  const manualSet = $derived(new Set(s.build.manual_buffs ?? []));

  // ---- Add Other Buff: clickies/worn/procs/potions — NEVER auto-applied ----
  async function toggleOtherPanel() {
    addOtherOpen = !addOtherOpen;
    if (addOtherOpen && otherCatalog == null) {
      try { otherCatalog = await listOtherBuffs(); } catch (e) { s.error = String(e); }
    }
  }
  const otherSet = $derived(new Set((s.build.other_buffs ?? []).map((n) => n.toLowerCase())));
  function toggleOther(name: string) {
    const cur = s.build.other_buffs ?? [];
    const has = cur.some((n) => n.toLowerCase() === name.toLowerCase());
    s.build.other_buffs = has
      ? cur.filter((n) => n.toLowerCase() !== name.toLowerCase())
      : [...cur, name];
  }
  const KIND_LABEL: Record<string, string> = {
    CLICK: "Item clickies", WORN: "Worn effects", PROC: "Weapon procs",
    CONSUMABLE: "Potions & consumables",
  };
  const otherGroups = $derived.by(() => {
    const groups: Record<string, OtherBuffRow[]> = {};
    for (const r of otherCatalog ?? []) {
      if (otherSearch && !r.name.toLowerCase().includes(otherSearch.toLowerCase())
          && !r.line.toLowerCase().includes(otherSearch.toLowerCase())) continue;
      (groups[r.source_kind] ??= []).push(r);
    }
    return groups;
  });

  // ---- cap field + over-cap toggle ----
  let capDraft = $state<number | null>(null);
  const cap = $derived(capDraft ?? plan?.buff_slot_cap ?? 30);
  $effect(() => {
    if (capDraft != null && plan?.buff_slot_cap === capDraft) capDraft = null;
  });
  async function commitCap(raw: string | number) {
    // an emptied/garbage field must NOT persist buff_slot_cap=1 app-wide — treat
    // non-finite input as "leave the cap as it is"
    const n = typeof raw === "string" ? Number(raw.trim()) : raw;
    if (raw === "" || !Number.isFinite(n)) { capDraft = null; return; }
    const v = Math.min(60, Math.max(1, Math.round(n)));
    capDraft = v;
    try {
      await setFormula("buff_slot_cap", String(v), false);
      s.resolveNonce++;
    } catch (e) {
      s.error = String(e);
      capDraft = null;
    }
  }

  const BADGE: Record<string, string> = {
    AUTO: "auto", MANUAL: "manual", OTHER: "other", EXTERNAL: "external", BARD: "bard",
  };

  // ---- combat modes (stances + invocations, eqlbuilds snapshot; display-only v1) ----
  let modes = $state<Mode[]>([]);
  $effect(() => {
    listModes().then((m) => (modes = m)).catch((e) => (s.error = String(e)));
  });
  const stances = $derived(modes.filter((m) => m.kind === "stance"));
  const invocations = $derived(modes.filter((m) => m.kind === "invocation"));
  const activeStance = $derived(stances.find((m) => m.id === s.build.stance) ?? null);
  const activeInvocation = $derived(
    invocations.find((m) => m.id === s.build.invocation) ?? null
  );

  /** gold chip when the resolver's why mentions a spell tier (e.g. "tier 3"). */
  function tierChip(why: string): string | null {
    const m = /tier\s*\d+/i.exec(why);
    if (m) return m[0];
    return /tier/i.test(why) ? "tier" : null;
  }
</script>

{#if !plan}
  <p class="muted">Waiting for calculation…</p>
{:else}
  <div class="header">
    <p class="slots">
      Buff slots: <strong>{plan.buff_slots_used} / {plan.buff_slot_cap}</strong>
      {#if !s.build.allow_over_cap && plan.buff_slots_used >= plan.buff_slot_cap}<span class="full">CAP REACHED</span>{/if}
      {#if s.build.allow_over_cap && plan.buff_slots_used > plan.buff_slot_cap}<span class="over">OVER CAP</span>{/if}
    </p>
    <label class="capfield" title="max simultaneous buffs (engine-enforced unless over-cap)">
      cap
      <input
        type="number" min="1" max="60"
        value={cap}
        onchange={(e) => commitCap(e.currentTarget.value)}
      />
    </label>
    <label class="strict" title="theorycrafting: keep everything, never trim to the cap">
      <input type="checkbox" bind:checked={s.build.allow_over_cap} />
      Allow over cap
    </label>
    <label class="strict" title="only count buffs from spells scribed in your Spell Manager, or enabled item sources you actually wear">
      <input type="checkbox" bind:checked={s.build.strict_buffs} />
      Strict availability
    </label>
  </div>

  <div class="toolbar">
    <button class="tbtn" onclick={() => (addClassOpen = !addClassOpen)}>➕ Add Class Buff</button>
    <button class="tbtn" onclick={toggleOtherPanel}>🧪 Add Other Buff</button>
    <button class="tbtn" onclick={clearAll} title="disable every active buff (re-enable individually)">Clear All</button>
    <button class="tbtn warn" onclick={restoreDefaults}
      title="back to the recommended automatic loadout — clears manual picks, enabled item sources, external casts and disabled buffs">
      Restore Defaults
    </button>
    <label class="strict showall">
      <input type="checkbox" bind:checked={showAllLines} />
      show all available (not just active)
    </label>
  </div>

  <p class="autonote">
    The default loadout is what your character can cast <strong>alone</strong>: strongest
    rank per line from your unlocked classes at level {s.build.level}, tier-scaled.
    {#if s.result?.locked_classes?.length}
      <span class="locked">{s.result.locked_classes.join("/")} locked until higher level — contributing nothing yet.</span>
    {/if}
    Items, potions and other players' buffs only count when you add them deliberately.
  </p>

  {#if addClassOpen}
    <div class="pickpanel">
      <div class="pickhead">
        <strong>Add / pin a class buff</strong>
        <span class="dim">picking a member pins it for its line (MANUAL) — even a lower rank for testing</span>
        <button class="tbtn" onclick={() => (addClassOpen = false)}>Close</button>
      </div>
      <ul class="picklist">
        {#each classChoices as c, i (c.line + "#" + c.spell_id + "#" + i)}
          <li>
            <button
              class="pickbtn"
              class:current={c.current}
              class:pinned={manualSet.has(c.spell_id)}
              onclick={() => pickManual(c.line, c.spell_id)}
              title={c.current ? "currently chosen for this line" : "pin as this line's pick"}
            >
              <span class="pline">{c.line}</span>
              <span class="pname">{c.name}</span>
              <span class="pval">{c.value != null ? `+${c.value}` : ""}</span>
              {#if manualSet.has(c.spell_id)}<span class="pbadge">pinned</span>{/if}
            </button>
          </li>
        {:else}
          <li class="dim">no castable line members — pick classes and a level first</li>
        {/each}
      </ul>
      {#if (s.build.manual_buffs ?? []).length > 0}
        <p class="pinconfirm">
          ✓ {(s.build.manual_buffs ?? []).length} pinned — counted in the plan and shown
          in <strong>Active buffs</strong> above with a <span class="badge b-manual">manual</span> badge.
          Click a pinned row again to unpin it.
        </p>
        <button class="tbtn" onclick={() => (s.build.manual_buffs = [])}>unpin all manual picks</button>
      {/if}
    </div>
  {/if}

  {#if addOtherOpen}
    <div class="pickpanel">
      <div class="pickhead">
        <strong>Add other buffs</strong>
        <span class="dim">clickies, worn effects, procs, potions — never applied automatically; enable each deliberately</span>
        <button class="tbtn" onclick={() => (addOtherOpen = false)}>Close</button>
      </div>
      <input class="othersearch" placeholder="search other buffs…" bind:value={otherSearch} />
      {#if otherCatalog == null}
        <p class="dim">loading…</p>
      {:else}
        {#each Object.entries(otherGroups) as [kind, rows] (kind)}
          <h4 class="kindhead">{KIND_LABEL[kind] ?? kind} ({rows.length})</h4>
          <ul class="picklist">
            {#each rows as r, i (kind + "#" + r.name + "#" + i)}
              <li>
                <label class="otherrow">
                  <input
                    type="checkbox"
                    checked={otherSet.has(r.name.toLowerCase())}
                    onchange={() => toggleOther(r.name)}
                  />
                  <span class="pname">{r.name}</span>
                  <span class="pline">{r.line}</span>
                  <span class="pval">{r.value != null ? `+${r.value}` : ""}</span>
                  {#if r.source_items}<span class="psrc">{r.source_items}</span>{/if}
                </label>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="dim">no matches</p>
        {/each}
        <p class="dim exthint">
          Buffs from OTHER PLAYERS (any class) are marked on the Spell Info tab — the Cast
          column. External casts show here with an <span class="badge b-external">external</span> badge.
        </p>
      {/if}
    </div>
  {/if}

  <section>
    <h2>Active buffs ({plan.active.length})</h2>
    {#if plan.active.length === 0}
      <p class="muted">none</p>
    {:else}
      <ul class="active">
        <!-- keyed: a disabled buff's row (with its dirtied checkbox) must be DESTROYED,
             not recycled onto the next buff — an unkeyed each made the eye checkbox
             land unchecked on the wrong buff, and "fixing" it disabled that buff too -->
        {#each plan.active as b (b.name)}
          <li>
            <input
              class="eye" type="checkbox" checked
              title="turn this buff off — its whole line is removed (no weaker rank refills)"
              onchange={() => toggleBuffOff(b.lines)}
            />
            <span class="name {statusClass(b.status)}">{b.name}</span>
            {#if BADGE[b.source_tag]}
              <span class="badge b-{BADGE[b.source_tag]}"
                title={b.source_tag === "AUTO" ? "automatically chosen class buff"
                     : b.source_tag === "MANUAL" ? "manually pinned class spell"
                     : b.source_tag === "OTHER" ? "deliberately enabled item/consumable"
                     : b.source_tag === "EXTERNAL" ? "cast on you by another player"
                     : "maintained by the group's bard"}>{BADGE[b.source_tag]}</span>
            {/if}
            <span class="val">+{b.total_value}</span>
            {#if b.duration}<span class="dur">{b.duration}</span>{/if}
            <span class="lines">{b.lines.join(", ")}</span>
            {#if tierChip(b.why)}<span class="tierchip">{tierChip(b.why)}</span>{/if}
            <span class="why">{b.why}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  {#if modes.length > 0}
    <section class="modes">
      <h2>Combat modes — stance &amp; invocation</h2>
      <p class="modenote">
        One stance and one invocation can be active. Descriptions carry the real numbers
        (eqlbuilds.com snapshot) — <strong>not yet folded into the stat totals</strong>.
      </p>
      <div class="modegrid">
        <div class="modecol">
          <label class="modelabel" for="stance-select">Stance</label>
          <select
            id="stance-select"
            value={s.build.stance ?? ""}
            onchange={(e) => (s.build.stance = e.currentTarget.value || null)}
          >
            <option value="">(none)</option>
            {#each stances as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
          </select>
          {#if activeStance}
            {#if activeStance.message}<p class="modemsg">{activeStance.message}</p>{/if}
            <p class="modedesc">{activeStance.description}</p>
          {/if}
        </div>
        <div class="modecol">
          <label class="modelabel" for="invocation-select">Invocation</label>
          <select
            id="invocation-select"
            value={s.build.invocation ?? ""}
            onchange={(e) => (s.build.invocation = e.currentTarget.value || null)}
          >
            <option value="">(none)</option>
            {#each invocations as m (m.id)}<option value={m.id}>{m.name}</option>{/each}
          </select>
          {#if activeInvocation}
            {#if activeInvocation.message}<p class="modemsg">{activeInvocation.message}</p>{/if}
            <p class="modedesc">{activeInvocation.description}</p>
          {/if}
        </div>
      </div>
    </section>
  {/if}

  {#if disabledLines.length > 0}
    <section>
      <h2>Turned off ({disabledLines.length})</h2>
      <ul class="disabled">
        {#each disabledLines as line (line)}
          <li>
            <input
              class="eye" type="checkbox"
              title="turn this line back on"
              onchange={() => reEnableLine(line)}
            />
            <span class="name">{line}</span>
            <span class="offhint">off — no buff from this line at all</span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  <section>
    <h2>Rejected ({plan.rejected.length})</h2>
    {#if plan.rejected.length === 0}
      <p class="muted">none</p>
    {:else}
      <ul class="rejected">
        {#each plan.rejected as b}
          <li>
            <span class="name">{b.name}</span>
            <span class="line">{b.line}</span>
            <span class="reason">{b.reason}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  {#if showAllLines}
    <section>
      <h2>All lines ({plan.lines.length})</h2>
      <LinesTable lines={plan.lines} />
    </section>
  {/if}
{/if}

<style>
  .header { display: flex; gap: 1rem; align-items: center; flex-wrap: wrap; }
  .strict { display: flex; gap: .4rem; align-items: center; color: #c9b26a; font-size: .85rem; cursor: pointer; }
  .strict input { cursor: pointer; }
  .slots { font-size: 1.05rem; background: #1c1f26; display: inline-block; padding: .4rem .8rem; border-radius: 8px; margin: 0; }
  .slots strong { color: #fc6; }
  .full { color: #f88; font-size: .72rem; margin-left: .6rem; border: 1px solid #a44; border-radius: 4px; padding: 1px 5px; }
  h2 { font-size: .95rem; margin: 1rem 0 .4rem; color: #bcd; }
  .muted { color: #667; }
  ul { list-style: none; padding: 0; margin: 0; }
  li { display: flex; gap: .6rem; align-items: baseline; padding: 2px 0; border-bottom: 1px solid #20242b; }
  .name { min-width: 220px; }
  .status { font-size: .68rem; border: 1px solid currentColor; border-radius: 4px; padding: 0 4px; }
  .val { color: #fc6; font-weight: 600; min-width: 3rem; text-align: right; }
  .lines { color: #89a; font-size: .75rem; flex: 1; }
  .why { color: #6c9; font-size: .72rem; }
  .tierchip {
    color: #c9b26a; border: 1px solid #8a7440; border-radius: 3px;
    background: rgba(201, 178, 106, .08);
    font-size: .65rem; padding: 0 4px; font-variant: small-caps; letter-spacing: .05em;
  }
  .rejected .name { color: #cab; }
  .rejected .line { color: #89a; font-size: .75rem; min-width: 190px; }
  .rejected .reason { color: #c86; font-size: .75rem; }
  .eye { cursor: pointer; margin: 0; }
  .disabled li { opacity: .6; }
  .disabled .name { color: #99a; min-width: 220px; }
  .offhint { color: #667; font-size: .72rem; }

  /* ---- toolbar + pick panels ---- */
  .toolbar { display: flex; gap: .5rem; align-items: center; flex-wrap: wrap; margin: .5rem 0; }
  .tbtn {
    background: #22262d; color: #c9b26a; border: 1px solid #8a7440; border-radius: 6px;
    padding: 3px 10px; cursor: pointer; font-size: .78rem;
  }
  .tbtn:hover { border-color: #c9b26a; color: #e6d08a; }
  .tbtn.warn { color: #da5; border-color: #543; }
  .tbtn.warn:hover { color: #fc8; border-color: #da5; }
  .showall { margin-left: auto; }
  .capfield { display: flex; gap: .35rem; align-items: center; color: #c9b26a; font-size: .8rem; }
  .capfield input {
    width: 3.4rem; background: #1c1f26; color: #fc6; border: 1px solid #333;
    border-radius: 6px; padding: 2px 6px; text-align: right;
  }
  .over { color: #fa8; font-size: .72rem; margin-left: .6rem; border: 1px solid #a63; border-radius: 4px; padding: 1px 5px; }
  .autonote { color: #987; font-size: .76rem; max-width: 760px; margin: .2rem 0 .6rem; }
  .autonote strong { color: #c9b26a; }
  .autonote .locked { color: #da5; }
  .pickpanel {
    background: #171a20; border: 1px solid #2a2f38; border-radius: 8px;
    padding: .55rem .8rem; margin: 0 0 .7rem;
  }
  .pickhead { display: flex; gap: .7rem; align-items: baseline; margin-bottom: .4rem; flex-wrap: wrap; }
  .pickhead strong { color: #c9b26a; }
  .pickhead .tbtn { margin-left: auto; }
  .picklist { max-height: 34vh; overflow: auto; }
  .pickbtn {
    display: flex; gap: .6rem; align-items: baseline; width: 100%; text-align: left;
    background: none; border: none; color: #cbd; padding: 2px 4px; cursor: pointer; font: inherit;
    font-size: .8rem;
  }
  .pickbtn:hover { background: #22262d; }
  .pickbtn.current .pname { color: #6c9; }
  .pickbtn.pinned .pname { color: #c9b26a; }
  .pline { color: #89a; font-size: .74rem; min-width: 200px; }
  .pname { flex: 1; }
  .pval { color: #fc6; }
  .pbadge { color: #c9b26a; font-size: .66rem; border: 1px solid #8a7440; border-radius: 4px; padding: 0 4px; }
  .psrc { color: #667; font-size: .7rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 280px; }
  .otherrow { display: flex; gap: .6rem; align-items: baseline; cursor: pointer; padding: 1px 0; font-size: .8rem; }
  .othersearch {
    width: 100%; box-sizing: border-box; padding: 5px 8px; background: #1c1f26;
    border: 1px solid #333; color: #e6e6e6; border-radius: 6px; margin-bottom: .4rem;
  }
  .kindhead { color: #c9b26a; font-variant: small-caps; letter-spacing: .06em; margin: .5rem 0 .2rem; font-size: .8rem; }
  .exthint { margin-top: .5rem; }
  .pinconfirm { color: #6c9; font-size: .76rem; margin: .4rem 0 .3rem; }
  .pinconfirm strong { color: #c9b26a; }
  .dim { color: #667; font-size: .74rem; }

  /* ---- source badges + duration on active rows ---- */
  .badge { font-size: .64rem; border-radius: 4px; padding: 0 5px; border: 1px solid currentColor; font-variant: small-caps; letter-spacing: .04em; }
  .b-auto { color: #6c9; }
  .b-manual { color: #c9b26a; }
  .b-other { color: #c9c; }
  .b-external { color: #fa8; }
  .b-bard { color: #6af; }
  .dur { color: #778; font-size: .72rem; min-width: 5.5rem; }

  /* ---- combat modes (stances & invocations) ---- */
  .modes { background: #171a20; border: 1px solid #2a2f38; border-radius: 8px;
           padding: .6rem .8rem; margin-top: .8rem; }
  .modes h2 { margin: 0 0 .2rem; color: #c9b26a; font-variant: small-caps; letter-spacing: .06em; }
  .modenote { color: #987; font-size: .74rem; margin: 0 0 .5rem; }
  .modenote strong { color: #da5; }
  .modegrid { display: grid; grid-template-columns: 1fr 1fr; gap: .8rem; }
  @media (max-width: 900px) { .modegrid { grid-template-columns: 1fr; } }
  .modelabel { display: block; color: #9ab; font-size: .78rem; margin-bottom: .2rem; }
  .modes select {
    width: 100%; background: #1c1f26; color: #e6e6e6; border: 1px solid #333;
    border-radius: 6px; padding: 4px 6px;
  }
  .modemsg { color: #6c9; font-size: .74rem; font-style: italic; margin: .35rem 0 .15rem; }
  .modedesc { color: #cbd; font-size: .78rem; line-height: 1.5; margin: 0; }
</style>
