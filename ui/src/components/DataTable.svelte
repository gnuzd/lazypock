<script>
  let { columns = [], rows = [], onrowclick } = $props();
</script>

<div class="table-wrapper">
  <table class="table">
    <thead>
      <tr>
        {#each columns as col}
          <th class={col.class}>{col.label}</th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#each rows as row}
        <tr>
          {#each columns as col}
            <td
              class={col.class}
              role={onrowclick ? "button" : undefined}
              onclick={onrowclick ? () => onrowclick(row) : undefined}
            >
              {col.render ? col.render(row) : row[col.key]}
            </td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .table-wrapper {
    overflow-x: auto;
    border: var(--border) solid var(--color-base-300);
    border-radius: var(--radius-box);
    background: var(--color-base-100);
  }
  .table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--font-size-sm);
  }
  .table :global(th) {
    padding: 10px 14px;
    text-align: left;
    font-weight: 600;
    font-size: var(--font-size-xs);
    color: var(--color-base-content);
    opacity: 0.6;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    background: var(--color-base-200);
    border-bottom: var(--border) solid var(--color-base-300);
    white-space: nowrap;
  }
  .table :global(td) {
    padding: 8px 14px;
    border-bottom: var(--border) solid var(--color-base-200);
    max-width: 250px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .table :global(tbody tr) {
    transition: background var(--animation-speed-fast);
  }
  .table :global(tbody tr:hover) {
    background: var(--color-base-200);
  }
  .table :global(tbody tr:last-child td) {
    border-bottom: none;
  }
</style>
