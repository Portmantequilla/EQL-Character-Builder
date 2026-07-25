<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import type { AppState } from "../state.svelte";
  import {
    listLoadoutFiles, readSocials, writeSocials, exportSocialsDesktop,
    type LoadoutFile, type Social,
  } from "../api";
  import {
    MACRO_LIBRARY, MACRO_CATEGORIES, MACRO_TOKENS, MAX_LINE_LEN,
    unknownCommand, type LibraryMacro,
  } from "../macroLibrary";

  // macros live in the game's own LO1 file, not in the build — accept (ignore) app state for parity
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  let { s: _s }: { s: AppState } = $props();

  const MAX_LINES = 5;
  const PAGES = 10;   // in-game Socials grid: 10 pages
  const BUTTONS = 12; // × 12 buttons (mirrors the hotbar layout)
  // best-effort swatches for the game's 0-15 color index (exact hue is set in-game)
  const EQ_COLORS = [
    "#e8e8e8", "#38b0ff", "#ff4d4d", "#46d846", "#ffd23f", "#e86cff", "#3fe0e0", "#ff9838",
    "#9aa8ff", "#c07bff", "#8fe08f", "#ffb84d", "#ff7bbf", "#7bc0ff", "#b8b8b8", "#8a8a8a",
  ];
  const swatch = (c: number) => EQ_COLORS[((c % 16) + 16) % 16];

  // ---- file + editor state ----
  let files = $state<LoadoutFile[]>([]);
  let selectedPath = $state<string | null>(null);
  let character = $state<string | null>(null);
  let city = $state<string | null>(null);
  let socials = $state<Social[]>([]);
  let dirty = $state(false);
  let busy = $state(false);
  let err = $state<string | null>(null);
  let flash = $state<string | null>(null);
  let openIdx = $state<number | null>(null);
  let confirmWrite = $state(false);
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;
  let showLibrary = $state(false);
  let libCat = $state<string | null>(null); // category filter (null = all)

  function note(msg: string) { flash = msg; setTimeout(() => (flash = msg === flash ? null : flash), 4500); }

  async function refreshFiles() {
    try { files = await listLoadoutFiles(); } catch (e) { err = String(e); }
  }
  refreshFiles();

  async function loadFile(path: string) {
    busy = true; err = null;
    try {
      const list = await readSocials(path);
      socials = list;
      selectedPath = path;
      const f = files.find((x) => x.path === path);
      character = f?.character ?? null;
      city = f?.city ?? null;
      openIdx = null; dirty = false;
      note(`Loaded ${list.length} macro${list.length === 1 ? "" : "s"} from ${character ?? "the file"}.`);
    } catch (e) { err = String(e); }
    finally { busy = false; }
  }
  async function browseFile() {
    try {
      const path = await open({ filters: [{ name: "LO1 / macros INI", extensions: ["ini"] }] });
      if (typeof path === "string") await loadFile(path);
    } catch (e) { err = String(e); }
  }

  // ---- edit model ----
  function nextFreeSlot(): { page: number; button: number } {
    const used = new Set(socials.map((s) => `${s.page}:${s.button}`));
    for (let p = 1; p <= PAGES; p++)
      for (let b = 1; b <= BUTTONS; b++)
        if (!used.has(`${p}:${b}`)) return { page: p, button: b };
    return { page: 1, button: 1 };
  }
  function addMacro() {
    const { page, button } = nextFreeSlot();
    socials = [...socials, { page, button, name: "New Macro", color: 0, lines: ["", "", "", "", ""].slice(0, 1) }];
    openIdx = socials.length - 1;
    dirty = true;
  }
  function addFromLibrary(m: LibraryMacro) {
    // context-aware: if a macro is OPEN for editing, append this template's command line(s) to
    // it (fill the empty slots) — that's what you want when building one macro up. If nothing is
    // open, drop it in as a NEW macro (and don't auto-open, so several in a row stay separate).
    if (openIdx != null && socials[openIdx]) {
      const sc = socials[openIdx];
      const existing = sc.lines.filter((l) => l.trim() !== "");
      const room = MAX_LINES - existing.length;
      if (room <= 0) { note(`“${sc.name || "this macro"}” already has ${MAX_LINES} lines — no room to add.`); return; }
      const toAdd = m.lines.filter((l) => l.trim() !== "").slice(0, room);
      sc.lines = [...existing, ...toAdd];
      dirty = true;
      const dropped = m.lines.filter((l) => l.trim() !== "").length - toAdd.length;
      note(`Added ${toAdd.length} line${toAdd.length === 1 ? "" : "s"} to “${sc.name || "this macro"}”${dropped ? ` (${dropped} didn't fit)` : ""}.`);
    } else {
      const { page, button } = nextFreeSlot();
      socials = [...socials, { page, button, name: m.name, color: m.color, lines: [...m.lines] }];
      dirty = true;
      note(`Added “${m.name}” as a new macro. Open it to edit, or keep clicking to add more.`);
    }
  }
  // the macro the library will add lines to (null = library creates new macros)
  const openMacroName = $derived(openIdx != null && socials[openIdx] ? (socials[openIdx].name || "this macro") : null);
  function deleteMacro(i: number) {
    socials = socials.filter((_, idx) => idx !== i);
    if (openIdx === i) openIdx = null;
    else if (openIdx != null && openIdx > i) openIdx -= 1;
    dirty = true;
  }
  const lineAt = (sc: Social, i: number) => sc.lines[i] ?? "";
  function setLine(sc: Social, i: number, val: string) {
    const lines = [...sc.lines];
    while (lines.length <= i) lines.push("");
    lines[i] = val;
    while (lines.length && lines[lines.length - 1] === "") lines.pop();
    sc.lines = lines;
    dirty = true;
  }
  function setField<K extends keyof Social>(sc: Social, k: K, v: Social[K]) { sc[k] = v; dirty = true; }
  const slotTaken = (page: number, button: number, self: Social) =>
    socials.some((s) => s !== self && s.page === page && s.button === button);

  // ---- write / export ----
  async function writeToGame() {
    if (!selectedPath) { err = "Load a character's LO1 file first (so the rest of the file is preserved)."; return; }
    if (!confirmWrite) {
      confirmWrite = true;
      clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => (confirmWrite = false), 3500);
      return;
    }
    clearTimeout(confirmTimer); confirmWrite = false;
    busy = true; err = null;
    try {
      const r = await writeSocials(selectedPath, socials);
      dirty = false;
      const bak = r.backup ? ` · backup: ${r.backup.split(/[\\/]/).pop()}` : "";
      note(`Wrote ${r.count} macro${r.count === 1 ? "" : "s"} to ${r.path.split(/[\\/]/).pop()}${bak}`);
    } catch (e) { err = String(e); }
    finally { busy = false; }
  }
  async function exportDesktop() {
    busy = true; err = null;
    try {
      const label = character ?? "macros";
      const path = await exportSocialsDesktop(label, socials);
      note(`Exported a shareable macros fragment → ${path.split(/[\\/]/).pop()}`);
    } catch (e) { err = String(e); }
    finally { busy = false; }
  }

  const sorted = $derived(
    socials.map((s, idx) => ({ s, idx })).sort((a, b) =>
      a.s.page - b.s.page || a.s.button - b.s.button)
  );
</script>

<div class="wrap">
  <header>
    <div class="titlerow">
      <h2>Macros</h2>
      <span class="sub">the game's <strong>socials</strong> · <code>[Socials]</code> in <code>&lt;Char&gt;_&lt;city&gt;_LO1.ini</code></span>
    </div>

    <div class="toolbar">
      <label class="fld"><span>Open character</span>
        <select disabled={busy} onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; if (v) loadFile(v); }}>
          <option value="">{files.length ? "— pick a character file —" : "— none found —"}</option>
          {#each files as f (f.path)}
            <option value={f.path} selected={f.path === selectedPath}>{f.character ?? "?"} · {f.city ?? "?"}</option>
          {/each}
        </select>
      </label>
      <button class="btn" onclick={browseFile} disabled={busy}>Browse…</button>
      <div class="spacer"></div>
      <button class="btn" onclick={exportDesktop} disabled={busy || socials.length === 0} title="save a shareable [Socials] fragment to your Desktop">Export to Desktop</button>
      <button class="btn save" class:arm={confirmWrite} onclick={writeToGame} disabled={busy || !selectedPath}
        title="replace the [Socials] section of the loaded LO1 file — everything else is preserved, a .bak backup is saved first">
        {confirmWrite ? "click again to write" : dirty ? "Write to game*" : "Write to game"}
      </button>
    </div>

    {#if err}<p class="err">{err}</p>{/if}
    {#if flash}<p class="flash">{flash}</p>{/if}
    <p class="tip">
      Macros are stored in your character's LO1 settings file. <strong>Load the character first</strong>
      so a Write preserves the rest of that file (spell sets, hotbars, sound); a <code>.bak</code>
      backup is always saved. Close EQL before writing. Each macro is a button with a name, a color,
      and up to {MAX_LINES} slash-command lines.
    </p>
  </header>

  <div class="listhead">
    <h3>Macros ({socials.length})</h3>
    <div class="headbtns">
      <button class="btn lib" class:on={showLibrary} onclick={() => (showLibrary = !showLibrary)}>📖 Library</button>
      <button class="btn add" onclick={addMacro} disabled={busy}>＋ New macro</button>
    </div>
  </div>

  {#if showLibrary}
    <div class="library">
      <div class="libbar">
        {#if openMacroName}
          <span class="liblbl mode">↳ adding lines to <strong>{openMacroName}</strong> — click a macro to append its command(s)</span>
        {:else}
          <span class="liblbl">click a macro to add it as a <strong>new</strong> button</span>
        {/if}
        <button class="catchip" class:on={libCat === null} onclick={() => (libCat = null)}>All</button>
        {#each MACRO_CATEGORIES as c (c)}
          <button class="catchip" class:on={libCat === c} onclick={() => (libCat = c)}>{c}</button>
        {/each}
      </div>
      <div class="libgrid">
        {#each MACRO_LIBRARY.filter((m) => libCat === null || m.category === libCat) as m (m.category + m.name)}
          <button class="libcard" onclick={() => addFromLibrary(m)} title={m.note ?? m.lines.join(" / ")}>
            <span class="cdot" style="background:{swatch(m.color)}"></span>
            <span class="libname">{m.name}</span>
            <span class="libcmd">{m.lines.join("  ·  ")}</span>
            <span class="libplus">＋</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}

  {#if socials.length === 0}
    <p class="empty">No macros yet. Load a character's file above, add one with “New macro”, or pick from the 📖 Library.</p>
  {:else}
    <ul class="mlist">
      {#each sorted as { s, idx } (idx)}
        <li class:open={openIdx === idx}>
          <button class="mrow" onclick={() => (openIdx = openIdx === idx ? null : idx)}>
            <span class="slot" title="Socials page {s.page}, button {s.button}">P{s.page}·B{s.button}</span>
            <span class="cdot" style="background:{swatch(s.color)}" title="color index {s.color}"></span>
            <span class="mname">{s.name || "(unnamed)"}</span>
            <span class="mprev">{s.lines.filter((l) => l).slice(0, 2).join("  ·  ")}{s.lines.filter((l) => l).length > 2 ? " …" : ""}</span>
            <span class="lc">{s.lines.filter((l) => l).length}/{MAX_LINES}</span>
          </button>

          {#if openIdx === idx}
            <div class="editor">
              <div class="erow">
                <label class="ef grow"><span>Name</span>
                  <input value={s.name} maxlength="24" oninput={(e) => setField(s, "name", (e.currentTarget as HTMLInputElement).value)} />
                </label>
                <label class="ef"><span>Color</span>
                  <span class="colorpick">
                    <span class="cdot big" style="background:{swatch(s.color)}"></span>
                    <select value={s.color} onchange={(e) => setField(s, "color", +(e.currentTarget as HTMLSelectElement).value)}>
                      {#each Array.from({ length: 16 }, (_, i) => i) as ci (ci)}<option value={ci}>{ci}</option>{/each}
                    </select>
                  </span>
                </label>
                <label class="ef"><span>Page</span>
                  <select value={s.page} onchange={(e) => setField(s, "page", +(e.currentTarget as HTMLSelectElement).value)}>
                    {#each Array.from({ length: PAGES }, (_, i) => i + 1) as p (p)}<option value={p}>{p}</option>{/each}
                  </select>
                </label>
                <label class="ef"><span>Button</span>
                  <select value={s.button} onchange={(e) => setField(s, "button", +(e.currentTarget as HTMLSelectElement).value)}>
                    {#each Array.from({ length: BUTTONS }, (_, i) => i + 1) as b (b)}<option value={b}>{b}</option>{/each}
                  </select>
                </label>
                <button class="del" title="delete this macro" onclick={() => deleteMacro(idx)}>Delete</button>
              </div>
              {#if slotTaken(s.page, s.button, s)}
                <p class="warn">⚠ Another macro already sits on page {s.page}, button {s.button} — one will overwrite the other in-game. Pick a free slot.</p>
              {/if}
              <div class="lines">
                {#each Array.from({ length: MAX_LINES }, (_, i) => i) as i (i)}
                  {@const val = lineAt(s, i)}
                  {@const bad = unknownCommand(val)}
                  <div class="lrow">
                    <span class="lnum">{i + 1}</span>
                    <input class="lineinput" class:over={val.length > MAX_LINE_LEN} class:badcmd={!!bad}
                      placeholder={i === 0 ? "/command …" : "(optional)"}
                      value={val} spellcheck="false"
                      oninput={(e) => setLine(s, i, (e.currentTarget as HTMLInputElement).value)} />
                  {#if val}
                    <span class="lmeta">
                      {#if bad}<span class="lbad" title="'/{bad}' isn't a known EQL command — typo? (a line with no leading slash is fine; it's said to chat)">/{bad}?</span>{/if}
                      <span class="lcount" class:over={val.length > MAX_LINE_LEN}>{val.length}</span>
                    </span>
                  {/if}
                  </div>
                {/each}
              </div>
              <p class="tokens">
                {#each MACRO_TOKENS as t, ti (t.token)}<code>{t.token}</code> {t.meaning}{ti < MACRO_TOKENS.length - 1 ? " · " : ""}{/each}
                · <code>/pause N</code> = wait N tenths of a second · up to {MAX_LINES} lines, ~{MAX_LINE_LEN} chars each
              </p>
            </div>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .wrap { display: flex; flex-direction: column; gap: .6rem; }
  header { display: flex; flex-direction: column; gap: .4rem; }
  .titlerow { display: flex; align-items: baseline; gap: .6rem; flex-wrap: wrap; }
  h2 { margin: 0; font-size: 1.05rem; color: #e6e6e6; }
  .sub { color: #667; font-size: .76rem; }
  .sub code { background: #12151c; border: 1px solid #2a2f38; border-radius: 4px; padding: 0 4px; }
  .toolbar { display: flex; align-items: flex-end; gap: .5rem; flex-wrap: wrap; }
  .spacer { flex: 1 1 auto; }
  .fld { display: flex; flex-direction: column; gap: 2px; font-size: .7rem; color: #89a; }
  .fld select { background: #12151c; color: #cdd; border: 1px solid #333; border-radius: 6px; padding: 5px 8px; font: inherit; font-size: .82rem; min-width: 12rem; }
  .btn { background: #22262d; color: #cbd; border: 1px solid #3a3f4a; border-radius: 6px; padding: 6px 12px; cursor: pointer; font: inherit; font-size: .8rem; white-space: nowrap; }
  .btn:hover:not(:disabled) { background: #2a2f38; }
  .btn:disabled { opacity: .5; cursor: default; }
  .btn.save { border-color: #46c; color: #9cf; }
  .btn.save.arm { background: #3a2a12; color: #f0b040; border-color: #b8791f; }
  .btn.add { border-color: #2f4a2f; color: #8d8; }
  .err { color: #f77; font-size: .8rem; margin: 0; }
  .flash { color: #7c9; font-size: .8rem; margin: 0; }
  .tip { color: #8a94a6; font-size: .72rem; line-height: 1.5; margin: .1rem 0 0; max-width: 780px; }
  .tip strong { color: #cbd; } .tip code { background: #0e1117; border: 1px solid #2a2f38; border-radius: 4px; padding: 0 4px; }

  .listhead { display: flex; align-items: center; justify-content: space-between; }
  .listhead h3 { margin: 0; font-size: .9rem; color: #c9b26a; }
  .empty { color: #667; font-size: .82rem; font-style: italic; }

  .mlist { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
  .mlist li { background: #12151c; border: 1px solid #232833; border-radius: 8px; overflow: hidden; }
  .mlist li.open { border-color: #3a4560; }
  .mrow { display: flex; align-items: center; gap: .6rem; width: 100%; text-align: left; background: none; border: none; color: #cbd; cursor: pointer; font: inherit; padding: 7px 10px; }
  .mrow:hover { background: #171b22; }
  .slot { color: #7a8496; font-size: .68rem; font-variant-numeric: tabular-nums; min-width: 3.2rem; }
  .cdot { width: 11px; height: 11px; border-radius: 50%; flex: 0 0 auto; box-shadow: inset 0 0 0 1px rgba(0,0,0,.4); }
  .cdot.big { width: 15px; height: 15px; }
  .mname { color: #e6e6e6; font-size: .86rem; min-width: 8rem; }
  .mprev { color: #6a7280; font-size: .74rem; flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .lc { color: #566; font-size: .68rem; }

  .editor { padding: 6px 10px 10px; border-top: 1px solid #20242b; }
  .erow { display: flex; align-items: flex-end; gap: .5rem; flex-wrap: wrap; }
  .ef { display: flex; flex-direction: column; gap: 2px; font-size: .68rem; color: #89a; }
  .ef.grow { flex: 1 1 12rem; }
  .ef input, .ef select { background: #0e1117; color: #cdd; border: 1px solid #333; border-radius: 6px; padding: 5px 7px; font: inherit; font-size: .82rem; }
  .ef.grow input { width: 100%; box-sizing: border-box; }
  .colorpick { display: flex; align-items: center; gap: .35rem; }
  .colorpick select { width: 3.6rem; }
  .del { background: #3a1a1a; color: #f88; border: 1px solid #a33; border-radius: 6px; padding: 5px 12px; cursor: pointer; font: inherit; font-size: .76rem; }
  .del:hover { background: #4a2020; }
  .warn { color: #da5; font-size: .72rem; margin: .4rem 0 0; }
  .lines { display: flex; flex-direction: column; gap: 3px; margin-top: .5rem; }
  .lrow { display: flex; align-items: center; gap: .4rem; }
  .lnum { color: #566; font-size: .7rem; width: 1rem; text-align: right; }
  .lineinput { flex: 1 1 auto; background: #0e1117; color: #cbd; border: 1px solid #2a2f38; border-radius: 6px; padding: 5px 8px; font: inherit; font-size: .8rem; font-family: ui-monospace, monospace; }
  .lineinput:focus { border-color: #46c; outline: none; }
  .lineinput.badcmd { border-color: #7a5a2a; }
  .lineinput.over { border-color: #a33; }
  .lmeta { display: flex; align-items: center; gap: .35rem; min-width: 3rem; justify-content: flex-end; }
  .lbad { color: #da5; font-size: .68rem; cursor: help; }
  .lcount { color: #566; font-size: .66rem; font-variant-numeric: tabular-nums; }
  .lcount.over { color: #f77; font-weight: 600; }
  .tokens { color: #7a8496; font-size: .68rem; margin: .5rem 0 0; line-height: 1.5; }
  .tokens code { background: #0e1117; border: 1px solid #2a2f38; border-radius: 3px; padding: 0 3px; color: #9ab; }

  /* ---- library ---- */
  .headbtns { display: flex; gap: .4rem; }
  .btn.lib.on { background: #141a24; color: #c9b26a; border-color: #8a7440; }
  .library { background: #10131a; border: 1px solid #262b33; border-radius: 8px; padding: .5rem .6rem; }
  .libbar { display: flex; align-items: center; gap: .35rem; flex-wrap: wrap; margin-bottom: .5rem; }
  .liblbl { color: #89a; font-size: .72rem; margin-right: .2rem; }
  .liblbl strong { color: #cbd; }
  .liblbl.mode { color: #8c9; }
  .liblbl.mode strong { color: #adb; }
  .catchip { background: #171b22; color: #99a; border: 1px solid #2a2f38; border-radius: 12px; padding: 2px 10px; cursor: pointer; font: inherit; font-size: .72rem; }
  .catchip.on { background: #141a24; color: #9cf; border-color: #46c; }
  .libgrid { display: grid; grid-template-columns: repeat(auto-fill, minmax(210px, 1fr)); gap: 5px; }
  .libcard { display: flex; align-items: center; gap: .4rem; text-align: left; background: #161a21; border: 1px solid #232833; border-radius: 6px; padding: 5px 8px; cursor: pointer; font: inherit; color: #cbd; }
  .libcard:hover { background: #1c2430; border-color: #3a4560; }
  .libname { color: #e6e6e6; font-size: .8rem; min-width: 4.5rem; }
  .libcmd { color: #6a7280; font-size: .68rem; font-family: ui-monospace, monospace; flex: 1 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .libplus { color: #8d8; font-size: .9rem; }
</style>
