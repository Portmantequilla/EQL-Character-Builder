<script lang="ts">
  import type { LineResolution } from "./api";
  import { statusClass } from "./format";

  let { lines }: { lines: LineResolution[] } = $props();
</script>

<table class="lines">
  <thead>
    <tr><th>Line</th><th>Statistic</th><th>Chosen</th><th class="num">Value</th><th>Why</th><th>Rejected</th></tr>
  </thead>
  <tbody>
    {#each lines as l}
      <tr class:empty={!l.chosen}>
        <td class="line">{l.line}</td>
        <td class="stat">{l.statistic ?? "—"}</td>
        {#if l.chosen}
          <td class={statusClass(l.chosen.status)}>{l.chosen.name}</td>
          <td class="num val">{l.chosen.value ?? "?"}</td>
          <td class="why">{l.chosen.why}</td>
        {:else}
          <td class="none">—</td><td class="num"></td><td></td>
        {/if}
        <td class="rej">{l.rejected_reason ?? ""}</td>
      </tr>
    {/each}
  </tbody>
</table>

<style>
  table { border-collapse: collapse; width: 100%; font-size: .8rem; }
  th { text-align: left; color: #89a; font-weight: 600; padding: 3px 8px 3px 0; border-bottom: 1px solid #333; }
  th.num { text-align: right; }
  td { padding: 2px 8px 2px 0; border-bottom: 1px solid #20242b; }
  td.num { text-align: right; }
  tr.empty { opacity: .45; }
  .line { color: #89a; }
  .stat { color: #9ab; }
  .val { color: #fc6; font-weight: 600; }
  .why { color: #6c9; font-size: .72rem; }
  .rej { color: #c86; font-size: .72rem; }
  .none { color: #667; font-style: italic; }
</style>
