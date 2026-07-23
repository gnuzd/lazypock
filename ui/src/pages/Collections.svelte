<script>
  import { api } from "../lib/api.js";
  import SlideOver from "../components/SlideOver.svelte";

  let { collectionName, on404 = () => {} } = $props();

  let collection = $state(null);
  let records = $state([]);
  let loading = $state(false);
  let showNewRecord = $state(false);

  $effect(() => {
    if (collectionName) load(collectionName);
  });

  async function load(id) {
    if (!id) return;
    loading = true;
    try {
      const [coll, recs] = await Promise.all([
        api.getCollection(id),
        api.listRecords(id, { page: "1", perPage: "50" }),
      ]);
      collection = coll;
      records = recs.items || [];
    } catch (e) {
      if (e.status === 404) {
        on404(id);
      } else {
        console.error("load collection:", e);
      }
    } finally {
      loading = false;
    }
  }

  function formatValue(field, value) {
    if (value == null) return "—";
    if (field.type === "bool") return value ? "✓" : "✗";
    if (typeof value === "object") return JSON.stringify(value).slice(0, 50);
    return String(value);
  }
</script>

<div class="page">
  <div class="page-header">
    <div class="breadcrumbs">
      <span class="breadcrumbs-item">Collections</span>
      <span class="breadcrumbs-separator">/</span>
      <span class="breadcrumbs-item">{collection?.name ?? "..."}</span>
    </div>
    <div style="display:flex;gap:8px">
      <button class="btn btn-outline btn-sm">API Preview</button>
      <button class="btn btn-primary btn-sm" onclick={() => showNewRecord = true}>+ New Record</button>
    </div>
  </div>

  <div class="page-content">
    {#if loading}
      <div class="empty-state">
        <p>Loading...</p>
      </div>
    {:else if collection}
      <div class="tab-bar">
        <button class="tab active">Records</button>
        <button class="tab">Schema</button>
        <button class="tab">API Preview</button>
      </div>

      <div style="margin-top:var(--spacing-sm)">
        {#if records.length === 0}
          <div class="empty-state">
            <h3>No records yet</h3>
            <p>Create your first record to get started.</p>
          </div>
        {:else}
          <div class="table-wrapper">
            <table class="table">
              <thead>
                <tr>
                  <th>ID</th>
                  {#each collection.schema ?? [] as field}
                    <th>{field.name}</th>
                  {/each}
                </tr>
              </thead>
              <tbody>
                {#each records as rec}
                  <tr>
                    <td><code>{rec.id?.slice(0, 8)}...</code></td>
                    {#each collection.schema ?? [] as field}
                      <td>{formatValue(field, rec[field.name])}</td>
                    {/each}
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<SlideOver bind:show={showNewRecord} title="New Record">
  <p>Record form coming next</p>
</SlideOver>

<style>
  .page {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--spacing-sm) var(--spacing);
    border-bottom: var(--border) solid var(--color-base-300);
    flex-shrink: 0;
  }
  .page-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing-sm) var(--spacing);
  }
  .tab-bar {
    display: flex;
    border-bottom: var(--border) solid var(--color-base-300);
    gap: 2px;
  }
  .tab {
    padding: 8px 16px;
    font-size: var(--font-size-sm);
    font-weight: 500;
    color: var(--color-base-content);
    opacity: 0.6;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    cursor: pointer;
    transition: all var(--animation-speed-fast);
  }
  .tab:hover {
    opacity: 1;
    background: var(--color-base-200);
  }
  .tab.active {
    opacity: 1;
    border-bottom-color: var(--color-primary);
    color: var(--color-primary);
  }
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    flex: 1;
    min-height: 200px;
    color: var(--color-base-content);
    opacity: 0.4;
    gap: 8px;
  }
  .empty-state h3 {
    font-size: var(--font-size-base);
    font-weight: 500;
  }
  .empty-state p {
    font-size: var(--font-size-sm);
  }
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
  .table :global(tbody tr:hover) {
    background: var(--color-base-200);
  }
  .table :global(code) {
    font-family: var(--font-mono);
    font-size: var(--font-size-xs);
  }
</style>
