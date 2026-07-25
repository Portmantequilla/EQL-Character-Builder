<script lang="ts">
  import { appInfo, type AppInfo } from "./api";

  let { onclose }: { onclose: () => void } = $props();

  let info = $state<AppInfo | null>(null);
  appInfo().then((i) => (info = i)).catch(() => {});

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
  <div class="dlg" role="dialog" tabindex="-1" aria-modal="true" aria-label="About" onclick={(e) => e.stopPropagation()}>
    <div class="titlebar">About</div>

    <div class="body">
      <img class="mark" src="/brand/logo.png" alt="" width="96" height="96" />
      <h1>{info?.name ?? "EQL Character Builder"}</h1>
      <p class="ver">version {info?.version ?? "…"}</p>

      <p class="by">
        By <strong>{info?.author ?? "Bee Canyon Retro"}</strong>
      </p>

      <p class="copy">{info?.copyright ?? "© 2026 Bee Canyon Retro. All rights reserved."}</p>

      <h2>Legal</h2>
      <p class="legal">
        This program and its source code are the work of the author. All rights reserved.
        You may use it freely for personal, non-commercial play. It may not be sold,
        sublicensed, or redistributed for profit without written permission.
      </p>
      <p class="legal">
        <strong>Not affiliated with, endorsed by, or sponsored by</strong> Daybreak Game
        Company LLC, Darkpaw Games, or the EverQuest Legends team. <em>EverQuest</em> and
        <em>EverQuest Legends</em> are trademarks of their respective owners. Game names,
        item and spell data, and icons remain the property of their owners and are used
        here only to support play of the game.
      </p>
      <p class="legal">
        Game data is mirrored from the community wiki at <strong>eqlwiki.com</strong> and
        belongs to its contributors. Spell, skill, stance, and invocation data include a
        snapshot of the <strong>eqlbuilds.com</strong> community build planner, obtained
        via the MIT-licensed <strong>everquest-legends-mcp</strong> project (Arthur
        Sabintsev). Starting-attribute and multiclass combine rules cite
        <strong>eqltools.com</strong>. Base HP/mana curves and the item upgrade formulas
        come from <strong>Mosscovered Legend's EQL Stat Estimator</strong> (kosstile
        &amp; contributors: cactot, Dannuic, Gigglemage, morbes, Tubasnot, Walker).
        Values are community-maintained and may be wrong, incomplete, or out of date —
        always verify against the live game.
      </p>
      <p class="legal warranty">
        THIS SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
        IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
        FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHOR BE
        LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY ARISING FROM, OUT OF, OR IN
        CONNECTION WITH THE SOFTWARE OR ITS USE.
      </p>

      <h2>Built with</h2>
      <p class="legal">
        Tauri, Rust, Svelte, and SQLite — each under its own open-source license (MIT /
        Apache-2.0 / public domain).
      </p>

    </div>

    <div class="foot">
      <button class="ok" onclick={onclose}>Close</button>
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
    width: 620px; max-height: 82vh; display: flex; flex-direction: column;
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
  .mark { float: right; margin: -.2rem -.2rem .4rem .8rem; opacity: .95; }
  h1 { margin: 0; font-size: 1.15rem; color: #e6e6e6; }
  .ver { margin: .1rem 0 .8rem; color: #667; font-size: .78rem; }
  .by { margin: 0 0 .5rem; color: #cdd; line-height: 1.5; }
  .by strong { color: #e6e6e6; }
  .copy { margin: .2rem 0 1rem; color: #9ab; font-size: .8rem; }
  h2 {
    font-size: .72rem; color: #89a; text-transform: uppercase; letter-spacing: .1em;
    margin: 1rem 0 .35rem; border-bottom: 1px solid #262b33; padding-bottom: 3px;
  }
  .legal { margin: .35rem 0; color: #9ab; font-size: .76rem; line-height: 1.5; }
  .legal strong { color: #cdd; }
  .warranty { color: #778; font-size: .68rem; letter-spacing: .01em; }
  .foot {
    padding: .5rem .8rem; border-top: 1px solid #262b33; text-align: right;
    background: #12141a;
  }
  .ok {
    background: #22262d; color: #c9b26a; border: 1px solid #8a7440;
    border-radius: 6px; padding: 4px 16px; cursor: pointer;
  }
  .ok:hover { background: #2a2f38; }
</style>
