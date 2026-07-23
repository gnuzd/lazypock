<script>
  let { tabs = [], active = $bindable("") } = $props();
</script>

<nav class="tabs-header">
  {#each tabs as tab}
    <button
      class="tab-item"
      class:active={tab === active}
      onclick={() => active = tab}
    >
      <span class="txt">{tab}</span>
    </button>
  {/each}
</nav>

<style>
  .tabs-header {
    display: flex;
    border-bottom: var(--border) solid var(--color-base-300);
    gap: 0;
    margin: 0 calc(-1 * var(--spacing-sm));
    padding: 0 var(--spacing-sm);
  }
  .tab-item {
    flex: 1;
    padding: 8px 12px;
    font-size: var(--font-size-sm);
    font-weight: 500;
    color: var(--color-base-content);
    opacity: 0.6;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    transition: all var(--animation-speed-fast);
    text-align: center;
  }
  .tab-item:hover {
    opacity: 1;
    background: var(--color-base-200);
  }
  .tab-item.active {
    opacity: 1;
    border-bottom-color: var(--color-primary);
    color: var(--color-primary);
  }
</style>
