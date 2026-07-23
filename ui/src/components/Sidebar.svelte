<script>
  let { collections = [], activeName = "", onselect, onnew } = $props();

  let search = $state("");
  let filtered = $derived.by(() => {
    if (!search) return collections;
    const q = search.toLowerCase();
    return collections.filter(c => c.name?.toLowerCase().includes(q));
  });
</script>

<aside class="sidebar">
  <div class="sidebar-search">
    <input type="text" class="input input-sm" placeholder="Search..." bind:value={search} />
  </div>
  <nav class="sidebar-list">
    {#if filtered.length === 0}
      <div class="sidebar-empty">No collections</div>
    {:else}
      {#each filtered as coll (coll.id)}
        <button
          class="sidebar-item"
          class:active={coll.name === activeName}
          onclick={() => onselect(coll.name)}
        >
          <span class="icon">📁</span>
          <span class="truncate">{coll.name}</span>
          <span class="count">{coll.schema?.length ?? 0}</span>
        </button>
      {/each}
    {/if}
  </nav>
  <div class="sidebar-footer">
    <button class="btn btn-primary" style="width:100%" onclick={onnew}>+ New Collection</button>
  </div>
</aside>

<style>
  .sidebar {
    width: var(--sidebar-width);
    display: flex;
    flex-direction: column;
    border-right: var(--border) solid var(--color-base-300);
    background: var(--color-base-100);
    flex-shrink: 0;
    overflow: hidden;
  }
  .sidebar-search {
    padding: 8px;
    border-bottom: var(--border) solid var(--color-base-300);
  }
  .sidebar-search :global(.input) {
    font-size: var(--font-size-sm);
  }
  .sidebar-list {
    flex: 1;
    overflow-y: auto;
    padding: 4px 0;
  }
  .sidebar-empty {
    padding: 16px;
    text-align: center;
    opacity: 0.4;
    font-size: var(--font-size-sm);
  }
  .sidebar-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 12px;
    margin: 0 6px;
    width: calc(100% - 12px);
    border: none;
    border-radius: var(--radius-field);
    font-size: var(--font-size-sm);
    color: var(--color-base-content);
    background: none;
    cursor: pointer;
    text-align: left;
    transition: background var(--animation-speed-fast);
  }
  .sidebar-item:hover {
    background: var(--color-base-200);
  }
  .sidebar-item.active {
    background: var(--color-base-200);
    font-weight: 500;
  }
  .sidebar-item .icon {
    width: 16px;
    height: 16px;
    opacity: 0.6;
    flex-shrink: 0;
  }
  .sidebar-item .count {
    margin-left: auto;
    font-size: var(--font-size-xs);
    font-family: var(--font-mono);
    opacity: 0.4;
  }
  .sidebar-footer {
    padding: var(--spacing-sm);
    border-top: var(--border) solid var(--color-base-300);
  }
</style>
