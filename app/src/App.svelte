<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    getStatic, resolveBuild, queryItems, chooseForMe,
    saveBuild, listBuilds, loadBuild, deleteBuild, openDataFolder,
    exportBuild, importBuild, listAas,
    type BuildInput, type BuildSummary,
  } from "./lib/api";
  import { findAaByName, MNEMONIC_AA } from "./lib/aa";
  import AboutDialog from "./lib/AboutDialog.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import { AppState, newBuild } from "./lib/state.svelte";
  import OverviewTab from "./lib/tabs/OverviewTab.svelte";
  import EquipmentTab from "./lib/tabs/EquipmentTab.svelte";
  import BuffsTab from "./lib/tabs/BuffsTab.svelte";
  import SpellsTab from "./lib/tabs/SpellsTab.svelte";
  import FocusTab from "./lib/tabs/FocusTab.svelte";
  import ExaltationsTab from "./lib/tabs/ExaltationsTab.svelte";
  import SkillsTab from "./lib/tabs/SkillsTab.svelte";
  import AaTab from "./lib/tabs/AaTab.svelte";
  import SpellbookTab from "./lib/tabs/SpellbookTab.svelte";
  import StatsTab from "./lib/tabs/StatsTab.svelte";
  import PetTab from "./lib/tabs/PetTab.svelte";
  import FarmTab from "./lib/tabs/FarmTab.svelte";
  import LootFilterTab from "./lib/tabs/LootFilterTab.svelte";
  import MacrosTab from "./lib/tabs/MacrosTab.svelte";

  const s = new AppState();

  const TABS = ["Overview", "Stats", "Equipment", "Buffs", "Spell Info", "Spell Manager", "Focus Effects", "Exaltations", "Skills", "AA", "Pet", "Farm", "Loot Filter", "Macros"] as const;
  type Tab = (typeof TABS)[number];
  let active = $state<Tab>("Overview");

  let builds = $state<BuildSummary[]>([]);
  let selectedBuildId = $state<number | null>(null);
  let seed = $state(Date.now() % 100000);
  let flash = $state<string | null>(null);

  // ---- startup: static data + saved-build list ----
  // A fresh build defaults to what is LIVE in the game (level cap 50, in-era
  // expansions only). Everything stays adjustable past those defaults.
  let pristine = $state(true); // untouched New build -> safe to re-seed from static data
  getStatic()
    .then((d) => {
      s.staticData = d;
      if (pristine) s.build = newBuild({ eras: d.default_enabled_eras, levelCap: d.default_level_cap });
      if (d.item_count === 0) {
        s.error = `wiki database is missing or empty (looked at ${d.wiki_db_path}) — run the sync, then restart`;
      }
    })
    .catch((e) => { s.error = String(e); });
  refreshBuilds();

  // AA list is static wiki data — fetch once and hand it to every tab that needs it.
  listAas()
    .then((list) => { s.aas = list; })
    .catch((e) => { s.error = String(e); });

  // Legacy `aa_mnemonic_retention` -> the AA planner. The engine takes the MAX of the
  // two, so a saved build's old value would fight a lowered stepper; fold it into
  // `aa_ranks` (the one source of truth) the moment the AA list is available. Runs
  // again for every load/import. The field itself stays in the contract.
  $effect(() => {
    const mnem = findAaByName(s.aas, MNEMONIC_AA);
    if (!mnem) return;
    const legacy = s.build.aa_mnemonic_retention ?? 0;
    if (legacy <= 0) return;
    if ((s.build.aa_ranks?.[mnem.id] ?? 0) === 0) {
      s.build.aa_ranks = {
        ...(s.build.aa_ranks ?? {}),
        [mnem.id]: Math.min(legacy, mnem.max_rank),
      };
    }
    s.build.aa_mnemonic_retention = 0;
  });

  const liveEras = $derived(s.staticData?.default_enabled_eras ?? []);
  const levelCap = $derived(s.staticData?.default_level_cap ?? 50);

  async function refreshBuilds() {
    try { builds = await listBuilds(); } catch (e) { s.error = String(e); }
  }

  // ---- THE reactive pipeline: any build mutation -> ONE debounced resolve_build ----
  // `s.resolveNonce` lets something OUTSIDE the build force a re-resolve (Settings or
  // the Spells tab's cap slider edits a formula -> Rust refreshes the engine snapshot
  // -> the same build resolves differently). Lives on AppState so any tab can bump it.
  let resolveGen = 0;
  $effect(() => {
    // deep read of the whole BuildInput registers deep dependencies
    const snapshot = $state.snapshot(s.build) as BuildInput;
    void s.resolveNonce; // tracked: bumping it re-runs the pipeline unchanged-build
    const gen = ++resolveGen;
    const t = setTimeout(async () => {
      s.resolving = true;
      try {
        const r = await resolveBuild(snapshot);
        if (gen === resolveGen) { s.result = r; s.error = null; }
      } catch (e) {
        if (gen === resolveGen) s.error = String(e);
      }
      if (gen === resolveGen) s.resolving = false;
    }, 150);
    return () => clearTimeout(t);
  });

  // ---- item list follows the class combo (gear browser + name lookups) ----
  let itemsGen = 0;
  $effect(() => {
    const classes = [...s.build.classes];
    const gen = ++itemsGen;
    if (classes.length === 0) { s.items = []; return; }
    queryItems(classes)
      .then((list) => { if (gen === itemsGen) s.items = list; })
      .catch((e) => { if (gen === itemsGen) s.error = String(e); });
  });

  // ---- header actions ----
  function toggleClass(c: string) {
    const cur = s.build.classes;
    // Three Magicians: while MAG is the only class picked, clicking it again stacks
    // another instead of deselecting; clicking at three clears. Every other combo
    // toggles normally. Intentional — see CONTRIBUTING.md.
    if (c === "MAG" && cur.length > 0 && cur.every((x) => x === "MAG")) {
      s.build.classes = cur.length < 3 ? [...cur, "MAG"] : [];
      return;
    }
    if (cur.includes(c)) s.build.classes = cur.filter((x) => x !== c);
    else if (cur.length < 3) s.build.classes = [...cur, c];
  }

  function showFlash(msg: string) {
    flash = msg;
    setTimeout(() => { if (flash === msg) flash = null; }, 2500);
  }

  async function onSave() {
    try {
      const id = await saveBuild($state.snapshot(s.build) as BuildInput);
      selectedBuildId = id;
      await refreshBuilds();
      showFlash(`saved #${id}`);
    } catch (e) { s.error = String(e); }
  }

  async function onLoad() {
    if (selectedBuildId == null) return;
    try {
      s.build = await loadBuild(selectedBuildId);
      pristine = false;
      showFlash("loaded");
    } catch (e) { s.error = String(e); }
  }

  async function onDelete() {
    if (selectedBuildId == null) return;
    try {
      await deleteBuild(selectedBuildId);
      selectedBuildId = null;
      await refreshBuilds();
      showFlash("deleted");
    } catch (e) { s.error = String(e); }
  }

  // ---- share: a build travels as Desktop/EQLBuilder Exports/<name>.eqlbuild.json ----
  async function onExportBuild() {
    try {
      const p = await exportBuild($state.snapshot(s.build) as BuildInput);
      showFlash(`exported to ${p}`);
    } catch (e) { s.error = String(e); }
  }

  async function onImportBuild() {
    try {
      const path = await open({ filters: [{ name: "EQL build", extensions: ["json"] }] });
      if (path == null || Array.isArray(path)) return; // cancelled / multi-select
      s.build = await importBuild(path);
      pristine = false;
      selectedBuildId = null;
      showFlash("build imported");
    } catch (e) { s.error = String(e); }
  }

  async function onChoose() {
    try {
      s.build = await chooseForMe(seed, s.build.level, [...s.build.classes],
                                  [...s.build.enabled_eras]);
      showFlash(`rolled with seed ${seed}`);
    } catch (e) { s.error = String(e); }
  }

  // ---- expansion toggle: empty enabled_eras = ALL on; materialize on first toggle ----
  const allEras = $derived(s.staticData?.eras ?? []);
  function eraOn(e: string): boolean {
    return s.build.enabled_eras.length === 0 || s.build.enabled_eras.includes(e);
  }
  function toggleEra(e: string) {
    const cur = s.build.enabled_eras.length === 0 ? [...allEras] : [...s.build.enabled_eras];
    const next = cur.includes(e) ? cur.filter((x) => x !== e) : [...cur, e];
    // all back on -> collapse to [] (canonical "everything" form)
    s.build.enabled_eras = next.length === allEras.length ? [] : next;
  }
  function erasAll() { s.build.enabled_eras = []; }
  function erasLive() { s.build.enabled_eras = [...liveEras]; }
  function erasClassicOnly() { s.build.enabled_eras = ["Classic"]; }

  function rerollSeed() { seed = Date.now() % 100000; }
  function onNew() {
    s.build = newBuild({ eras: liveEras, levelCap });
    selectedBuildId = null;
    pristine = true;
  }

  const classList = $derived(s.staticData?.classes ?? []);
  const isPickleWizard = $derived(
    s.build.classes.length === 3 && s.build.classes.every((c) => c === "MAG"),
  );
  const raceList = $derived(s.staticData?.races ?? []);

  // ---- native menu bar: the Rust side emits `menu` with the clicked item id ----
  let showAbout = $state(false);
  let showSettings = $state(false);
  const TAB_BY_MENU: Record<string, Tab> = {
    tab_overview: "Overview", tab_equipment: "Equipment", tab_buffs: "Buffs",
    tab_spells: "Spell Info", tab_spellbook: "Spell Manager", tab_stats: "Stats",
    tab_pet: "Pet", tab_farm: "Farm", tab_lootfilter: "Loot Filter", tab_macros: "Macros",
  };
  listen<string>("menu", async (e) => {
    const id = e.payload;
    if (TAB_BY_MENU[id]) { active = TAB_BY_MENU[id]; return; }
    switch (id) {
      case "new_build": onNew(); break;
      case "save_build": await onSave(); break;
      case "open_build":
        // the header dropdown is the picker; nudge the user to it if nothing is selected
        if (selectedBuildId != null) await onLoad();
        else showFlash("pick a saved build in the header dropdown, then File > Open");
        break;
      case "choose_for_me": await onChoose(); break;
      case "settings": showSettings = true; break;
      case "export_build": await onExportBuild(); break;
      case "import_build": await onImportBuild(); break;
      case "open_data_folder":
        try { await openDataFolder(); } catch (err) { s.error = String(err); }
        break;
      case "reload": location.reload(); break;
      case "about": showAbout = true; break;
      case "verification":
        active = "Stats";
        showFlash("hover the † markers — every unverified number carries its confidence note");
        break;
      case "wiki": showFlash("data source: eqlwiki.com (community wiki)"); break;
      // spellbook import/export live on the Spell Manager tab (they need its state)
      case "import_spellbook":
      case "export_spellbook":
        active = "Spell Manager";
        showFlash("use the Import / Export buttons on the Spell Manager toolbar");
        break;
    }
  });
</script>

<main>
  <!-- any header interaction means the build is no longer a pristine default; the
       handlers are a passive bubble-listener, not an interactive control of their own -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <header oninput={() => (pristine = false)} onclick={() => (pristine = false)}>
    <div class="toprow">
      <h1><img class="brand" src="/brand/logo-sm.png" alt="" width="22" height="22" />EQL Character Builder</h1>
      <input class="buildname" placeholder="build name" bind:value={s.build.name} />
      <button onclick={onNew}>New</button>
      <button class="primary" onclick={onSave}>Save</button>
      <select bind:value={selectedBuildId}>
        <option value={null}>— saved builds —</option>
        {#each builds as b (b.id)}
          <option value={b.id}>{b.name} — L{b.level ?? "?"} {b.classes.join("/")}</option>
        {/each}
      </select>
      <button disabled={selectedBuildId == null} onclick={onLoad}>Load</button>
      <button class="danger" disabled={selectedBuildId == null} onclick={onDelete}>Delete</button>
      <!-- sharing lives in its own group so the row keeps a sane wrap point -->
      <span class="share">
        <button class="small" title="write a shareable .eqlbuild.json to the Desktop"
                onclick={onExportBuild}>Export</button>
        <button class="small" title="load a shared .eqlbuild.json"
                onclick={onImportBuild}>Import</button>
      </span>
      {#if flash}<span class="flash">{flash}</span>{/if}
      {#if s.resolving}<span class="calc">calculating…</span>{/if}
    </div>

    <div class="classes">
      {#each classList as c (c)}
        <button
          class:on={s.build.classes.includes(c)}
          disabled={!s.build.classes.includes(c) && s.build.classes.length >= 3}
          onclick={() => toggleClass(c)}
        >{c}</button>
      {/each}
      {#if isPickleWizard}
        <span class="picked pickle" title="Three Magicians. Brine be upon ye.">Pickle Wizard</span>
      {:else}
        <span class="picked">{s.build.classes.length}/3 classes</span>
      {/if}
    </div>

    <div class="controls">
      <label>Race
        <select bind:value={s.build.race}>
          <option value={null}>—</option>
          {#each raceList as race (race)}<option value={race}>{race}</option>{/each}
        </select>
      </label>
      <label>Level
        <input type="range" min="1" max="60" bind:value={s.build.level} />
        <strong>{s.build.level}</strong>
        {#if s.build.level > levelCap}
          <span class="overcap" title="above the live level cap — planning ahead">
            &gt; cap {levelCap}
          </span>
        {/if}
      </label>
      <label>
        <input type="checkbox" bind:checked={s.build.bard_in_group} />
        bard in group
      </label>
      <span class="choose">
        <label>seed
          <input class="seed" type="number" min="0" max="99999" bind:value={seed} />
        </label>
        <button onclick={rerollSeed} title="new random seed">🎲</button>
        <button class="primary" onclick={onChoose}>Choose for me</button>
      </span>
    </div>

    <div class="eras">
      <span class="eralbl">Expansions</span>
      {#each allEras as e (e)}
        <button
          class="era"
          class:on={eraOn(e)}
          class:unreleased={!liveEras.includes(e)}
          title={liveEras.includes(e) ? "live in the game" : "not released yet (wiki: out of era)"}
          onclick={() => toggleEra(e)}
        >{e}{#if !liveEras.includes(e)}<span class="soon">·</span>{/if}</button>
      {/each}
      <button class="era quick" onclick={erasLive}>live only</button>
      <button class="era quick" onclick={erasAll}>all</button>
      <button class="era quick" onclick={erasClassicOnly}>classic only</button>
      {#if s.build.enabled_eras.length > 0}
        <span class="erahint">out-of-era gear goes inactive + leaves pickers</span>
      {/if}
    </div>

    {#if s.error}
      <p class="err">{s.error} — is <code>db/eql.db</code> present and synced?</p>
    {/if}
  </header>

  <nav class="tabs">
    {#each TABS as t (t)}
      <button class:on={active === t} onclick={() => (active = t)}>{t}</button>
    {/each}
  </nav>

  <!-- all tabs stay mounted so filters/pickers keep their state across switches -->
  <div class="pane" hidden={active !== "Overview"}><OverviewTab {s} /></div>
  <div class="pane" hidden={active !== "Equipment"}><EquipmentTab {s} /></div>
  <div class="pane" hidden={active !== "Buffs"}><BuffsTab {s} /></div>
  <div class="pane" hidden={active !== "Spell Info"}><SpellsTab {s} /></div>
  <div class="pane" hidden={active !== "Focus Effects"}><FocusTab {s} /></div>
  <div class="pane" hidden={active !== "Exaltations"}><ExaltationsTab {s} /></div>
  <div class="pane" hidden={active !== "Skills"}><SkillsTab {s} /></div>
  <div class="pane" hidden={active !== "AA"}><AaTab {s} /></div>
  <div class="pane" hidden={active !== "Spell Manager"}><SpellbookTab {s} /></div>
  <div class="pane" hidden={active !== "Stats"}><StatsTab {s} /></div>
  <div class="pane" hidden={active !== "Pet"}><PetTab {s} /></div>
  <div class="pane" hidden={active !== "Farm"}><FarmTab {s} /></div>
  <div class="pane" hidden={active !== "Loot Filter"}><LootFilterTab {s} /></div>
  <div class="pane" hidden={active !== "Macros"}><MacrosTab {s} /></div>

  {#if showAbout}
    <AboutDialog onclose={() => (showAbout = false)} />
  {/if}

  {#if showSettings}
    <SettingsDialog
      onclose={() => (showSettings = false)}
      onchanged={() => {
        s.resolveNonce++;
        // formulas feed StaticData too (tier pct, extraction tier) — keep the UI's
        // copy in sync so tooltips agree with the freshly refreshed engine snapshot
        getStatic().then((d) => (s.staticData = d)).catch(() => {});
      }}
    />
  {/if}
</main>

<style>
  :global(body) { margin: 0; font: 14px/1.4 system-ui, sans-serif; background: #14161a; color: #e6e6e6; }

  /* buff-member status colors, shared across tabs */
  :global(.st-self) { color: #6c9; }
  :global(.st-bard) { color: #6af; }
  :global(.st-item) { color: #c9c; }
  :global(.st-extcast) { color: #fa8; } /* cast on you by another player */
  :global(.st-ext)  { color: #999; }
  :global(.st-unk)  { color: #667; }

  main { padding: .8rem 1.2rem; }
  header { margin-bottom: .6rem; }

  .toprow { display: flex; gap: .5rem; align-items: center; flex-wrap: wrap; }
  h1 { font-size: 1.15rem; margin: 0 .6rem 0 0; display: flex; align-items: center; gap: .45rem; }
  .brand { display: block; }
  .buildname { width: 200px; padding: 5px 8px; background: #1c1f26; border: 1px solid #333; color: #e6e6e6; border-radius: 6px; }
  .flash { color: #6c9; font-size: .8rem; }
  .calc { color: #fc6; font-size: .8rem; }

  /* share group: kept together so the top row wraps between groups, not inside one */
  .share { display: inline-flex; gap: 4px; align-items: center; white-space: nowrap;
           padding-left: .5rem; margin-left: .1rem; border-left: 1px solid #2c313a; }
  button.small { padding: 3px 9px; font-size: .8rem; color: #c9b26a; border-color: #55482a; }
  button.small:hover:not(:disabled) { color: #e0cf92; border-color: #8a7440; }

  button { background: #22262d; color: #9aa; border: 1px solid #333; border-radius: 6px; padding: 4px 10px; cursor: pointer; font: inherit; }
  button:hover:not(:disabled) { color: #e6e6e6; border-color: #2a6; }
  button:disabled { opacity: .35; cursor: default; }
  button.primary { background: #2a6; color: #012; border-color: #2a6; font-weight: 600; }
  button.danger:hover:not(:disabled) { border-color: #a44; color: #f88; }

  select { background: #1c1f26; color: #e6e6e6; border: 1px solid #333; border-radius: 6px; padding: 4px 6px; max-width: 260px; }

  .classes { display: flex; flex-wrap: wrap; gap: 4px; margin: .55rem 0; align-items: center; }
  .classes button { padding: 3px 9px; }
  .classes button.on { background: #2a6; color: #012; border-color: #2a6; font-weight: 600; }
  .picked { color: #667; font-size: .75rem; margin-left: .4rem; }
  .picked.pickle { color: #8fbf46; font-weight: 600; letter-spacing: .04em; }

  .eras { display: flex; flex-wrap: wrap; gap: 4px; margin: .35rem 0 .55rem; align-items: center; }
  .eralbl { color: #89a; font-size: .72rem; text-transform: uppercase; letter-spacing: .08em; margin-right: .3rem; }
  .era { padding: 2px 8px; font-size: .75rem; background: #22262d; color: #556;
         border: 1px solid #2c313a; border-radius: 6px; cursor: pointer; }
  .era.on { background: #1d2b3a; color: #7bc; border-color: #35597a; }
  .era.quick { color: #997; border-style: dashed; }
  .era.quick:hover, .era:hover { border-color: #8a7440; }
  /* eras not yet released in-game read as "future" even when toggled on */
  .era.unreleased { font-style: italic; }
  .era.unreleased.on { background: #2b2437; color: #b9a; border-color: #5a4a72; }
  .soon { margin-left: 3px; color: #a86; }
  .erahint { color: #a86; font-size: .7rem; margin-left: .4rem; }
  .overcap { color: #f90; font-size: .7rem; margin-left: .3rem; }

  .controls { display: flex; gap: 1.2rem; align-items: center; flex-wrap: wrap; }
  .controls label { display: flex; gap: .4rem; align-items: center; color: #9ab; }
  .controls strong { color: #fc6; min-width: 1.6rem; }
  .controls input[type="range"] { width: 180px; }
  .choose { display: flex; gap: .4rem; align-items: center; margin-left: auto; }
  .seed { width: 70px; padding: 3px 6px; background: #1c1f26; border: 1px solid #333; color: #e6e6e6; border-radius: 6px; }

  .err { color: #f88; }

  .tabs { display: flex; gap: 2px; border-bottom: 1px solid #333; margin-bottom: .8rem; }
  .tabs button { border: 1px solid transparent; border-bottom: none; border-radius: 6px 6px 0 0; background: none; padding: 6px 14px; }
  .tabs button.on { background: #1c1f26; border-color: #333; color: #e6e6e6; font-weight: 600; }

  .pane[hidden] { display: none; }
</style>
