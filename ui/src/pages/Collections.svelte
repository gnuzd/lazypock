<script>
  import { api } from "../lib/api.js";
  import DataTable from "../components/DataTable.svelte";
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

  let columns = $derived.by(() => {
    const cols = [
      { key: "id", label: "ID", render: (r) => r.id?.slice(0, 8) + "..." },
    ];
    for (const field of collection?.schema ?? []) {
      cols.push({
        key: field.name,
        label: field.name,
        render: (r) => formatValue(field, r[field.name]),
      });
    }
    return cols;
  });
</script>

<div class="page">
  <div class="page-header">
    <div class="breadcrumbs">
      <span class="breadcrumbs-item">Collections</span>
      <span class="breadcrumbs-separator">/</span>
      <span class="breadcrumbs-item">{collection?.name ?? "..."}</span>
    </div>
    <button class="btn btn-primary btn-sm" onclick={() => showNewRecord = true}>+ New Record</button>
  </div>

  <div class="page-content">
    {#if loading}
      <div class="empty-state">
        <p>Loading...</p>
      </div>
    {:else if collection}
      <div style="margin-top:var(--spacing-sm)">
        <DataTable
          columns={columns}
          rows={records}
          emptyLabel="No records yet. Create your first record to get started."
          emptyActionLabel="+ New Record"
          onemptyaction={() => showNewRecord = true}
        />
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
  .empty-state p {
    font-size: var(--font-size-sm);
  }

</style>
