<script>
  let {
    columns = [],
    rows = [],
    onrowclick,
    emptyLabel = "",
    emptyActionLabel = "",
    onemptyaction
  } = $props();
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
      {#if rows.length === 0}
        <tr>
          <td class="empty-cell" colspan={columns.length}>
            {#if emptyLabel}
              <span class="empty-label">{emptyLabel}</span>
            {/if}
            {#if emptyActionLabel && onemptyaction}
              <button class="btn btn-primary btn-xs" onclick={onemptyaction}>{emptyActionLabel}</button>
            {/if}
          </td>
        </tr>
      {:else}
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
      {/if}
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
  .empty-cell {
    text-align: center;
    padding: 32px 16px !important;
    opacity: 0.5;
  }
  .empty-label {
    display: block;
    margin-bottom: 8px;
    font-size: var(--font-size-sm);
  }
</style>
