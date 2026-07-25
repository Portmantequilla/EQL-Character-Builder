<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import type { AppState } from "../state.svelte";
  import { iconUrl } from "../format";
  import {
    lfListFiles, lfRead, lfWrite, lfImportInventory,
    lfCatalogSearch, lfCatalogCount, lfWikiSearch,
    type LfEntry, type LfFile, type CatalogItem, type WikiPick,
  } from "../api";

  // the loot filter lives in the game's own files, not in the build — but every tab is
  // mounted as <Tab {s} />, so we accept (and ignore) the shared app state for parity.
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  let { s: _s }: { s: AppState } = $props();

  // disposition = the per-item FILTER_ID, one action per item. Values are the game's Loot
  // Filter columns in order: 1 Loot / 2 Merge / 3 Store / 4 Sell (Merge/Store/Sell all loot
  // the item first, then act). Matches the in-game Edit Loot Filters window.
  const DISPOSITIONS = [
    { id: 1, label: "Loot",  short: "Loot",  color: "#6ac", hint: "auto-loot to your inventory" },
    { id: 2, label: "Merge", short: "Merge", color: "#b58cff", hint: "auto-loot, then merge into the upgrade system — motes and low-tier fodder" },
    { id: 3, label: "Store", short: "Store", color: "#6c9", hint: "auto-loot, then store to your bank / depot — gear worth keeping" },
    { id: 4, label: "Sell",  short: "Sell",  color: "#c9b26a", hint: "auto-loot, then sell to a vendor — trash" },
  ];
  const dispOf = (id: number) => DISPOSITIONS.find((d) => d.id === id);

  // ---- file + editor state ----
  let files = $state<LfFile[]>([]);
  let scanDir = $state<string | null>(null);
  let character = $state("");
  let city = $state("");
  let selectedPath = $state<string | null>(null);
  let entries = $state<LfEntry[]>([]);
  let dirty = $state(false);
  let busy = $state(false);
  let err = $state<string | null>(null);
  let flash = $state<string | null>(null);
  let catalogCount = $state(0);

  // city dropdown suggestions: the classic EQL home cities, plus any city already used by a
  // filter in the folder (so the user's own set is always offered). Free text still allowed.
  const KNOWN_CITIES = [
    "qeynos", "freeport", "halas", "rivervale", "kaladim", "felwithe",
    "kelethin", "akanon", "neriak", "grobb", "oggok", "erudin", "paineel",
  ];
  const citySuggestions = $derived(
    Array.from(new Set([
      ...files.map((f) => f.city).filter((c): c is string => !!c),
      ...KNOWN_CITIES,
    ]))
  );

  function note(msg: string) { flash = msg; setTimeout(() => (flash = msg === flash ? null : flash), 4000); }
  // some game icon_ids have no locally-mirrored PNG — hide the broken image rather than show it
  const hideBroken = (ev: Event) => ((ev.currentTarget as HTMLImageElement).style.visibility = "hidden");

  async function refreshFiles() {
    try {
      const scan = await lfListFiles();
      files = scan.files; scanDir = scan.dir;
    } catch (e) { err = String(e); }
  }
  async function refreshCount() {
    try { catalogCount = await lfCatalogCount(); } catch { /* ignore */ }
  }
  // initial load
  refreshFiles();
  refreshCount();

  async function loadFile(path: string) {
    busy = true; err = null;
    try {
      const doc = await lfRead(path);
      entries = doc.entries;
      character = doc.character ?? character;
      city = doc.city ?? city;
      selectedPath = doc.path;
      dirty = false;
      await refreshCount(); // reading harvests the file's ids into the catalog
      note(`Loaded ${doc.entries.length} entries from ${character}'s ${city} filter.`);
    } catch (e) { err = String(e); }
    finally { busy = false; }
  }

  async function browseFile() {
    try {
      const path = await open({
        filters: [{ name: "Loot filter", extensions: ["ini"] }],
        defaultPath: scanDir ?? undefined,
      });
      if (typeof path === "string") await loadFile(path);
    } catch (e) { err = String(e); }
  }

  function newFilter() {
    entries = []; selectedPath = null; dirty = false;
    note("Started a new, empty filter. Set the character + city, add items, then Save.");
  }

  async function save() {
    err = null;
    if (!character.trim() || !city.trim()) { err = "Enter a character name and a city (they name the file)."; return; }
    const writable = entries.filter((e) => e.item_id > 0);
    const pending = entries.length - writable.length;
    busy = true;
    try {
      const path = await lfWrite(character.trim(), city.trim(), writable);
      selectedPath = path; dirty = false;
      await refreshFiles();
      note(pending > 0
        ? `Saved ${writable.length} entries. ${pending} pending item(s) with no game id yet were skipped.`
        : `Saved ${writable.length} entries to ${path.split(/[\\/]/).pop()}.`);
    } catch (e) { err = String(e); }
    finally { busy = false; }
  }

  async function importInventory() {
    try {
      const path = await open({
        filters: [{ name: "Inventory dump", extensions: ["txt"] }],
        defaultPath: scanDir ?? undefined,
      });
      if (typeof path !== "string") return;
      busy = true; err = null;
      const n = await lfImportInventory(path);
      await refreshCount();
      note(`Harvested ${n} item id(s) from that inventory into the picker catalog.`);
    } catch (e) { err = String(e); }
    finally { busy = false; }
  }

  // ---- working-list grouping ----
  const groups = $derived(
    DISPOSITIONS.map((d) => ({ ...d, items: entries.filter((e) => e.item_id > 0 && e.filter_id === d.id) }))
  );
  const pendingItems = $derived(entries.filter((e) => e.item_id <= 0));
  const unknownItems = $derived(
    entries.filter((e) => e.item_id > 0 && !DISPOSITIONS.some((d) => d.id === e.filter_id))
  );

  function setDisposition(entry: LfEntry, id: number) { entry.filter_id = id; dirty = true; }
  function removeEntry(entry: LfEntry) {
    entries = entries.filter((e) => e !== entry); dirty = true;
  }
  function clearAll() {
    if (entries.length === 0) return;
    entries = []; dirty = true; note("Cleared the working list (nothing written until you Save).");
  }

  // ---- picker ----
  let pickerSource = $state<"catalog" | "wiki">("catalog");
  let query = $state("");
  let addAs = $state(1); // default action new items get (Loot = neutral, just pick it up)
  let catalogResults = $state<CatalogItem[]>([]);
  let wikiResults = $state<WikiPick[]>([]);
  let searching = $state(false);
  let searchToken = 0;

  async function runSearch() {
    const q = query.trim();
    const token = ++searchToken;
    if (!q) { catalogResults = []; wikiResults = []; searching = false; return; }
    searching = true;
    try {
      if (pickerSource === "catalog") {
        const r = await lfCatalogSearch(q, 80);
        if (token === searchToken) catalogResults = r;
      } else {
        const r = await lfWikiSearch(q, 80);
        if (token === searchToken) wikiResults = r;
      }
    } catch (e) { err = String(e); }
    finally { if (token === searchToken) searching = false; }
  }
  let debounce: ReturnType<typeof setTimeout> | undefined;
  function onQuery() { clearTimeout(debounce); debounce = setTimeout(runSearch, 180); }
  function switchSource(src: "catalog" | "wiki") { pickerSource = src; runSearch(); }

  const inList = (itemId: number) => itemId > 0 && entries.some((e) => e.item_id === itemId);

  /** Add (or re-target) a real-id item. Duplicate id -> just move it to the chosen disposition. */
  function addReal(itemId: number, name: string, iconId: number | null, pageid: number | null) {
    const existing = entries.find((e) => e.item_id === itemId);
    if (existing) { existing.filter_id = addAs; dirty = true; note(`${name} re-set to ${dispOf(addAs)?.label}.`); return; }
    entries = [...entries, {
      item_id: itemId, filter_id: addAs, icon_id: iconId ?? 0, name,
      base_name: name, tier: 0, pageid,
    }];
    dirty = true;
  }

  /** Add a wiki item we have no real game id for: a PENDING row (item_id 0), excluded from
   *  the written file until an inventory import / in-game loot supplies its id. */
  function addPending(w: WikiPick) {
    const key = w.name.toLowerCase();
    if (pendingItems.some((e) => e.base_name.toLowerCase() === key)) { note(`${w.name} is already pending.`); return; }
    entries = [...entries, {
      item_id: 0, filter_id: addAs, icon_id: w.icon_id ?? 0, name: w.name,
      base_name: w.name, tier: 0, pageid: w.pageid,
    }];
    dirty = true;
    note(`${w.name} added as pending — it needs a real game id before it can be saved.`);
  }
</script>

<div class="wrap">
  <header>
    <div class="titlerow">
      <h2>Loot Filter</h2>
      <span class="sub">AdvLoot personal filter · <code>LF_&lt;Char&gt;_&lt;city&gt;.ini</code></span>
    </div>

    <div class="toolbar">
      <label class="fld">
        <span>Open filter</span>
        <select
          disabled={busy}
          onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; if (v) loadFile(v); }}
        >
          <option value="">{files.length ? "— pick a saved filter —" : "— none found —"}</option>
          {#each files as f (f.path)}
            <option value={f.path} selected={f.path === selectedPath}>
              {f.character ?? "?"} · {f.city ?? "?"} ({f.entry_count})
            </option>
          {/each}
        </select>
      </label>
      <button class="btn" onclick={browseFile} disabled={busy}>Browse…</button>
      <button class="btn" onclick={newFilter} disabled={busy}>New</button>
      <div class="spacer"></div>
      <label class="fld sm"><span>Character</span>
        <input bind:value={character} placeholder="Name" oninput={() => (dirty = true)} />
      </label>
      <label class="fld sm"><span>City</span>
        <input bind:value={city} placeholder="City" list="lf-cities" oninput={() => (dirty = true)} />
        <datalist id="lf-cities">
          {#each citySuggestions as c (c)}<option value={c}></option>{/each}
        </datalist>
      </label>
      <button class="btn save" onclick={save} disabled={busy}>{dirty ? "Save*" : "Save"}</button>
    </div>

    {#if err}<p class="err">{err}</p>{/if}
    {#if flash}<p class="flash">{flash}</p>{/if}
  </header>

  <div class="cols">
    <!-- LEFT: the working filter -->
    <section class="pane build">
      <div class="panehead">
        <h3>This filter ({entries.filter((e) => e.item_id > 0).length})</h3>
        {#if entries.length}<button class="link" onclick={clearAll}>clear all</button>{/if}
      </div>
      <p class="tip">One entry matches <strong>every tier</strong> of an item — the game keys on the
        item id, which doesn't change with "+N". Add "Keg Mallet" once and it covers +0 … +10.</p>

      {#if entries.length === 0}
        <p class="empty">No entries yet. Load an existing filter, or add items from the right →</p>
      {/if}

      {#each groups as g (g.id)}
        {#if g.items.length}
          <div class="group">
            <div class="grouphdr" style="--c:{g.color}">
              <span class="gdot"></span>{g.label} <span class="gcount">{g.items.length}</span>
              <span class="ghint">{g.hint}</span>
            </div>
            {#each g.items as e (e.item_id)}
              <div class="row">
                {#if e.icon_id}<img class="ico" src={iconUrl(e.icon_id)} alt="" onerror={hideBroken} />{:else}<span class="ico ph"></span>{/if}
                <span class="nm" title={`game id ${e.item_id}`}>{e.base_name || e.name}</span>
                <span class="id">#{e.item_id}</span>
                <select class="disp" onchange={(ev) => setDisposition(e, +(ev.currentTarget as HTMLSelectElement).value)}>
                  {#each DISPOSITIONS as d (d.id)}<option value={d.id} selected={d.id === e.filter_id}>{d.short}</option>{/each}
                </select>
                <button class="x" title="remove" onclick={() => removeEntry(e)}>×</button>
              </div>
            {/each}
          </div>
        {/if}
      {/each}

      {#if unknownItems.length}
        <div class="group">
          <div class="grouphdr" style="--c:#89a"><span class="gdot"></span>Other filter codes <span class="gcount">{unknownItems.length}</span>
            <span class="ghint">a value this planner doesn't label — preserved as-is</span></div>
          {#each unknownItems as e (e.item_id)}
            <div class="row">
              {#if e.icon_id}<img class="ico" src={iconUrl(e.icon_id)} alt="" onerror={hideBroken} />{:else}<span class="ico ph"></span>{/if}
              <span class="nm">{e.base_name || e.name}</span>
              <span class="id">#{e.item_id}</span>
              <span class="rawf">code {e.filter_id}</span>
              <button class="x" title="remove" onclick={() => removeEntry(e)}>×</button>
            </div>
          {/each}
        </div>
      {/if}

      {#if pendingItems.length}
        <div class="group pending">
          <div class="grouphdr" style="--c:#d95"><span class="gdot"></span>Pending — no game id yet <span class="gcount">{pendingItems.length}</span>
            <span class="ghint">kept for planning, but skipped when saving until an id is known</span></div>
          {#each pendingItems as e (e.base_name)}
            <div class="row">
              {#if e.icon_id}<img class="ico" src={iconUrl(e.icon_id)} alt="" onerror={hideBroken} />{:else}<span class="ico ph"></span>{/if}
              <span class="nm">{e.base_name}</span>
              <span class="id q">id?</span>
              <select class="disp" onchange={(ev) => setDisposition(e, +(ev.currentTarget as HTMLSelectElement).value)}>
                {#each DISPOSITIONS as d (d.id)}<option value={d.id} selected={d.id === e.filter_id}>{d.short}</option>{/each}
              </select>
              <button class="x" title="remove" onclick={() => removeEntry(e)}>×</button>
            </div>
          {/each}
          <p class="pendnote">Import this character's inventory (button on the right) or loot the item
            in-game, then re-add it — the real id fills in and it becomes saveable.</p>
        </div>
      {/if}
    </section>

    <!-- RIGHT: the picker -->
    <section class="pane pick">
      <div class="panehead">
        <h3>Add items</h3>
        <button class="btn tiny" onclick={importInventory} disabled={busy} title="harvest real game ids from a /outputfile inventory dump">Import inventory…</button>
      </div>

      <div class="srcrow">
        <button class="tab" class:on={pickerSource === "catalog"} onclick={() => switchSource("catalog")}>
          Known items <span class="badge">{catalogCount}</span>
        </button>
        <button class="tab" class:on={pickerSource === "wiki"} onclick={() => switchSource("wiki")}>All wiki items</button>
      </div>

      <div class="addasrow">
        <span>Add as:</span>
        {#each DISPOSITIONS as d (d.id)}
          <button class="chip" class:on={addAs === d.id} style="--c:{d.color}" title={d.hint} onclick={() => (addAs = d.id)}>{d.short}</button>
        {/each}
      </div>

      <input class="search" bind:value={query} oninput={onQuery} placeholder={pickerSource === "catalog" ? "search items with a known id…" : "search all wiki items…"} />

      {#if pickerSource === "catalog"}
        <p class="tip">
          Items the app has a <strong>real game id</strong> for — harvested from filters you load
          and inventory dumps you import. These add cleanly and save.
        </p>
        {#if searching}<p class="empty">searching…</p>
        {:else if query.trim() && catalogResults.length === 0}
          <p class="empty">Nothing in the catalog matches. Try "All wiki items", or import an inventory to grow the catalog.</p>
        {:else}
          <ul class="results">
            {#each catalogResults as r (r.game_item_id)}
              <li>
                {#if r.icon_id}<img class="ico" src={iconUrl(r.icon_id)} alt="" onerror={hideBroken} />{:else}<span class="ico ph"></span>{/if}
                <span class="nm">{r.name}</span>
                <span class="id">#{r.game_item_id}</span>
                {#if inList(r.game_item_id)}<span class="inlist">✓ in filter</span>{/if}
                <button class="add" onclick={() => addReal(r.game_item_id, r.name, r.icon_id, r.pageid)}>＋</button>
              </li>
            {/each}
          </ul>
        {/if}
      {:else}
        <p class="tip">
          Every wiki item. A <span class="ok">green id</span> means we already know its real game id
          (safe to add). <span class="warn">No id</span> means the game hasn't shown it to us yet —
          add it as <em>pending</em> and it saves once you loot it or import an inventory that has it.
        </p>
        {#if searching}<p class="empty">searching…</p>
        {:else if query.trim() && wikiResults.length === 0}
          <p class="empty">No wiki item matches “{query}”.</p>
        {:else}
          <ul class="results">
            {#each wikiResults as w (w.pageid)}
              <li>
                {#if w.icon_id}<img class="ico" src={iconUrl(w.icon_id)} alt="" onerror={hideBroken} />{:else}<span class="ico ph"></span>{/if}
                <span class="nm">{w.name}</span>
                {#if w.game_item_id}
                  <span class="id ok">#{w.game_item_id}</span>
                  {#if inList(w.game_item_id)}<span class="inlist">✓</span>{/if}
                  <button class="add" onclick={() => addReal(w.game_item_id!, w.name, w.icon_id, w.pageid)}>＋</button>
                {:else}
                  <span class="id warn">no id</span>
                  <button class="add pend" title="add as pending — needs a real game id before it can be saved" onclick={() => addPending(w)}>＋?</button>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
    </section>
  </div>
</div>

<style>
  .wrap { display: flex; flex-direction: column; gap: .6rem; height: 100%; }
  header { display: flex; flex-direction: column; gap: .4rem; }
  .titlerow { display: flex; align-items: baseline; gap: .6rem; }
  h2 { margin: 0; font-size: 1.05rem; color: #e6e6e6; }
  .sub { color: #667; font-size: .76rem; }
  .sub code { background: #12151c; border: 1px solid #2a2f38; border-radius: 4px; padding: 0 4px; }
  .toolbar { display: flex; align-items: flex-end; gap: .5rem; flex-wrap: wrap; }
  .spacer { flex: 1 1 auto; }
  .fld { display: flex; flex-direction: column; gap: 2px; font-size: .7rem; color: #89a; }
  .fld.sm input { width: 8.5rem; }
  .fld select, .fld input {
    background: #12151c; color: #cdd; border: 1px solid #333; border-radius: 6px;
    padding: 5px 8px; font: inherit; font-size: .82rem;
  }
  .btn {
    background: #22262d; color: #cbd; border: 1px solid #3a3f4a; border-radius: 6px;
    padding: 6px 12px; cursor: pointer; font: inherit; font-size: .8rem; white-space: nowrap;
  }
  .btn:hover:not(:disabled) { background: #2a2f38; }
  .btn:disabled { opacity: .5; cursor: default; }
  .btn.save { border-color: #46c; color: #9cf; }
  .btn.tiny { padding: 3px 8px; font-size: .72rem; }
  .err { color: #f77; font-size: .8rem; margin: 0; }
  .flash { color: #7c9; font-size: .8rem; margin: 0; }

  .cols { display: grid; grid-template-columns: 1fr 1fr; gap: .8rem; min-height: 0; flex: 1 1 auto; }
  .pane { background: #12151c; border: 1px solid #232833; border-radius: 8px; padding: .6rem .7rem; overflow-y: auto; }
  .panehead { display: flex; align-items: center; justify-content: space-between; gap: .5rem; }
  .panehead h3 { margin: 0; font-size: .9rem; color: #c9b26a; }
  .link { background: none; border: none; color: #789; cursor: pointer; font: inherit; font-size: .72rem; text-decoration: underline; }
  .tip { color: #8a94a6; font-size: .72rem; line-height: 1.45; margin: .3rem 0 .5rem; }
  .tip strong { color: #cbd; }
  .empty { color: #667; font-size: .78rem; font-style: italic; padding: .4rem 0; }

  .group { margin-bottom: .5rem; }
  .grouphdr { display: flex; align-items: center; gap: .4rem; font-size: .74rem; color: var(--c); padding: 3px 0; border-bottom: 1px solid #20242b; margin-bottom: 3px; }
  .gdot { width: 8px; height: 8px; border-radius: 50%; background: var(--c); display: inline-block; }
  .gcount { color: #667; font-size: .7rem; }
  .ghint { color: #566; font-size: .66rem; font-style: italic; margin-left: auto; text-align: right; }

  .row { display: flex; align-items: center; gap: .4rem; padding: 2px 0; }
  .ico { width: 26px; height: 26px; border-radius: 4px; flex: 0 0 auto; background: #0c0e13; }
  .ico.ph { border: 1px dashed #2a2f38; }
  .nm { flex: 1 1 auto; color: #cdd; font-size: .8rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .id { color: #566; font-size: .68rem; font-variant-numeric: tabular-nums; }
  .id.ok { color: #6c9; }
  .id.warn { color: #d95; }
  .id.q { color: #d95; }
  .rawf { color: #89a; font-size: .68rem; }
  .disp { background: #1a1e26; color: #bcd; border: 1px solid #333; border-radius: 5px; font-size: .72rem; padding: 2px 4px; }
  .x { background: none; border: none; color: #a55; cursor: pointer; font-size: 1rem; line-height: 1; padding: 0 4px; }
  .x:hover { color: #f77; }
  .pending .row { opacity: .92; }
  .pendnote { color: #a86; font-size: .68rem; font-style: italic; margin: .2rem 0 0; }

  .srcrow { display: flex; gap: .3rem; margin-bottom: .4rem; }
  .tab { flex: 1 1 0; background: #171b22; color: #89a; border: 1px solid #2a2f38; border-radius: 6px; padding: 5px; cursor: pointer; font: inherit; font-size: .78rem; }
  .tab.on { background: #141a24; color: #9cf; border-color: #46c; }
  .badge { background: #253; color: #9d8; border-radius: 8px; padding: 0 6px; font-size: .68rem; }
  .addasrow { display: flex; align-items: center; gap: .35rem; font-size: .72rem; color: #89a; margin-bottom: .4rem; }
  .chip { background: #171b22; color: #99a; border: 1px solid #2a2f38; border-radius: 12px; padding: 2px 10px; cursor: pointer; font: inherit; font-size: .72rem; }
  .chip.on { color: var(--c); border-color: var(--c); background: #12151c; }
  .search { width: 100%; box-sizing: border-box; background: #0e1117; color: #cdd; border: 1px solid #333; border-radius: 6px; padding: 6px 9px; font: inherit; font-size: .82rem; margin-bottom: .4rem; }
  .ok { color: #6c9; } .warn { color: #d95; }

  .results { list-style: none; margin: 0; padding: 0; }
  .results li { display: flex; align-items: center; gap: .4rem; padding: 3px 0; border-bottom: 1px solid #191d24; }
  .inlist { color: #6c9; font-size: .68rem; }
  .add { background: #1c2a1c; color: #8d8; border: 1px solid #2f4a2f; border-radius: 6px; cursor: pointer; font-size: .9rem; line-height: 1; padding: 3px 9px; }
  .add:hover { background: #253a25; }
  .add.pend { background: #2a2418; color: #da5; border-color: #4a3f2f; }
</style>
