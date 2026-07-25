<script lang="ts">
  import type { Item } from "./api";
  import { hideOnError, iconUrl, itemSummary } from "./format";

  let { items, level, onpick, placeholder = "search items…" }: {
    items: Item[];              // pre-filtered + pre-sorted candidates
    level: number;              // build level (for required-level badge)
    onpick: (item: Item) => void;
    placeholder?: string;
  } = $props();

  let q = $state("");

  const shown = $derived(
    items
      .filter((i) => !q || i.name.toLowerCase().includes(q.toLowerCase()))
      .slice(0, 200)
  );
</script>

<div class="itempicker">
  <input {placeholder} bind:value={q} />
  <ul>
    {#each shown as i (i.pageid)}
      <li>
        <button class="pick" onclick={() => onpick(i)}>
          <span class="ic">
            {#if i.icon_id != null}
              {#key i.icon_id}
                <img src={iconUrl(i.icon_id)} alt="" draggable="false" onerror={hideOnError} />
              {/key}
            {/if}
          </span>
          <span class="nm">{i.name}</span>
          {#if i.required_level != null && i.required_level > level}
            <span class="req">req {i.required_level}</span>
          {/if}
          {#if i.era}<span class="era">{i.era}</span>{/if}
          <span class="sum">{itemSummary(i)}</span>
        </button>
      </li>
    {/each}
    {#if shown.length === 0}
      <li class="none">no matches</li>
    {/if}
  </ul>
</div>

<style>
  .itempicker input {
    width: 100%; box-sizing: border-box; padding: 6px 8px; background: #1c1f26;
    border: 1px solid #333; color: #e6e6e6; border-radius: 6px; margin-bottom: .4rem;
  }
  ul { list-style: none; padding: 0; margin: 0; max-height: 40vh; overflow: auto; }
  li { border-bottom: 1px solid #20242b; }
  li.none { color: #667; font-style: italic; padding: 4px 0; }
  .pick {
    display: flex; gap: .5rem; align-items: center; width: 100%; text-align: left;
    background: none; border: none; color: #e6e6e6; padding: 2px 4px; cursor: pointer;
    font: inherit;
  }
  .pick:hover { background: #22262d; }
  .ic {
    width: 24px; height: 24px; flex-shrink: 0;
    background: #101318; border: 1px solid #2e3440; border-radius: 2px;
    display: flex; align-items: center; justify-content: center;
  }
  .ic img { width: 24px; height: 24px; image-rendering: pixelated; }
  .nm { flex-shrink: 0; }
  .req { color: #f88; font-size: .7rem; border: 1px solid #a44; border-radius: 4px; padding: 0 4px; }
  .era { color: #778; font-size: .7rem; }
  .sum { color: #89a; font-size: .72rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
