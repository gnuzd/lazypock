<script>
  import { api } from "../lib/api.js";
  import { store } from "../lib/store.svelte.js";
  import SlideOver from "../components/SlideOver.svelte";
  import FieldSettings from "../components/FieldSettings.svelte";
  import NewFieldButton from "../components/NewFieldButton.svelte";

  import RuleField from "./RuleField.svelte";

  let { show = $bindable(false), onsave } = $props();

  let name = $state("");
  let type = $state("base");
  let typeOpen = $state(false);
  let saving = $state(false);
  let error = $state("");
  let activeTab = $state("Fields");
  let fields = $state([]);
  let indexes = $state([]);
  let typeWrapper;
  let fieldsListEl = $state(null);

  // API rules
  let listRule = $state(null);
  let viewRule = $state(null);
  let createRule = $state(null);
  let updateRule = $state(null);
  let deleteRule = $state(null);
  let showRulesInfo = $state(false);

  function closestChild(parent, node) {
    if (!node || !node.parentNode) return null;
    if (node.parentNode == parent) return node;
    return closestChild(parent, node.parentNode);
  }

  $effect(() => {
    const el = fieldsListEl;
    if (!el) return;

    function clearDrag() {
      el.querySelectorAll("[data-dragstart]")
        .forEach((n) => n.dataset.dragstart = "false");
      el.querySelectorAll("[data-dragover]")
        .forEach((n) => n.dataset.dragover = "false");
    }
    function onDragStart(e) {
      e.dataTransfer.setData("text/plain", "");
      e.dataTransfer.effectAllowed = "move";
      const child = closestChild(el, e.target);
      if (child) child.dataset.dragstart = "true";
    }

    function onDragEnter(e) {
      el.querySelectorAll("[data-dragover]").forEach((n) => n.dataset.dragover = "false");
      const child = closestChild(el, e.target);
      if (child) child.dataset.dragover = "true";
    }
    function prevent(e) { e.preventDefault(); }
    function onDrop(e) {
      const from = el.querySelector("[data-dragstart]");
      const to = closestChild(el, e.target);
      const items = [...el.children].filter((c) => c.hasAttribute("data-sortable-child"));
      clearDrag();
      if (!from || !to || to == from) return;
      const fromIndex = items.indexOf(from);
      const toIndex = items.indexOf(to);
      if (fromIndex === -1 || toIndex === -1) return;
      const clone = [...fields];
      const [moved] = clone.splice(fromIndex, 1);
      clone.splice(toIndex, 0, moved);
      fields = clone;
    }

    el.addEventListener("dragstart", onDragStart);
    el.addEventListener("dragenter", onDragEnter);
    el.addEventListener("dragend", clearDrag);
    el.addEventListener("dragover", prevent);
    el.addEventListener("drop", onDrop);

    return () => {
      el.removeEventListener("dragstart", onDragStart);
      el.removeEventListener("dragenter", onDragEnter);
      el.removeEventListener("dragend", clearDrag);
      el.removeEventListener("dragover", prevent);
      el.removeEventListener("drop", onDrop);
    };
  });

  const collectionTypes = [
    { value: "base", icon: "ri-folder-2-line", label: "Base collection" },
    { value: "view", icon: "ri-table-line", label: "View collection" },
    { value: "auth", icon: "ri-group-line", label: "Auth collection" },
  ];


  function getTypeLabel(val) {
    return collectionTypes.find(t => t.value === val)?.label ?? val;
  }

  function slugify(val) {
    return (val || "")
      .toLowerCase()
      .replace(/[^a-z0-9_]+/g, "_")
      .replace(/^_|_$/g, "")
      .replace(/_+/g, "_");
  }

  function handleNameInput(e) {
    if (e.isComposing) return;
    const raw = e.target.value;
    const slugged = slugify(raw);
    if (slugged && slugged !== name) {
      name = slugged;
    } else if (!slugged) {
      name = raw;
    }
  }

  function handleNameCompositionEnd(e) {
    name = e.target.value;
  }

  async function handleSave() {
    if (!name.trim() || saving) return;
    saving = true;
    error = "";
    try {
      const payload = {
        name: name.trim(),
        type,
        fields: fields.filter(f => !f["@toDelete"]).map(f => {
          const clean = { ...f };
          delete clean.__focus;
          delete clean["@toDelete"];
          return clean;
        }),
        indexes: indexes.filter(Boolean),
        listRule,
        viewRule,
        createRule,
        updateRule,
        deleteRule,
      };
      const coll = await api.createCollection(payload);
      store.collections = [...store.collections, coll];
      store.activeCollectionName = coll.name;
      show = false;
      onsave?.(coll);
    } catch (e) {
      error = e.message || "Failed to create collection";
    } finally {
      saving = false;
    }
  }

  function handleKeydown(e) {
    if ((e.ctrlKey || e.metaKey) && e.key === "s") {
      e.preventDefault();
      handleSave();
    }
  }
</script>

<SlideOver bind:show>
  <!-- Header -->
  <header class="modal-header isolated">
    <div class="grid sm">
      <div class="col-12 flex">
        <h6 class="modal-title">
          <span>Create </span>
          <span>collection</span>
        </h6>
        <div class="flex-fill"></div>
      </div>
      <div class="col-12">
        <div class="fields">
          <div class="field">
            <label for="coll-name">Name</label>
            <input
              id="coll-name"
              type="text"
              name="name"
              required
              spellcheck="false"
              class="input"
              placeholder="e.g. posts"
              value={name}
              oninput={handleNameInput}
              oncompositionend={handleNameCompositionEnd}
              onkeydown={handleKeydown}
              autofocus
            />
          </div>
          <div class="field addon" bind:this={typeWrapper}>
            <button
              type="button"
              class="btn sm collection-type-select"
              class:outline={true}
              onclick={() => typeOpen = !typeOpen}
            >
              <span class="txt">Type: {getTypeLabel(type)}</span>
              <i class="ri-arrow-drop-down-line m-l-auto"></i>
            </button>
            {#if typeOpen}
              <div class="dropdown nowrap collection-type-dropdown open">
                {#each collectionTypes as ct}
                  <button
                    type="button"
                    class="dropdown-item"
                    class:active={ct.value === type}
                    onmousedown={() => { type = ct.value; typeOpen = false; }}
                  >
                    <i class={ct.icon}></i>
                    <span class="txt">{ct.label}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        </div>
      </div>
      <div class="col-12">
        <nav class="tabs-header equal-width">
          <button type="button" class="tab-item" class:active={activeTab === "Fields"} onclick={() => activeTab = "Fields"}>
            <span class="txt">Fields</span>
          </button>
          <button type="button" class="tab-item" class:active={activeTab === "API rules"} onclick={() => activeTab = "API rules"}>
            <span class="txt">API rules</span>
          </button>
        </nav>
      </div>
    </div>
  </header>

  <!-- Content -->
  <div class="modal-content">
    {#if activeTab === "Fields"}
      <div class="tab-content-wrapper" data-tab="Fields">
        <div class="collection-tab-content collection-fields-tab-content">
          <div class="collection-fields-list" bind:this={fieldsListEl}>
            {#each fields as field, i (field)}
              <div class="sortable-field-wrapper" data-sortable-child>
                <FieldSettings field={fields[i]} fieldIndex={i} />
              </div>
            {/each}
          </div>

          {#if fields.length === 0}
            <div class="empty-fields">
              <p>No fields configured. Click "Add Field" to start.</p>
            </div>
          {/if}

          <NewFieldButton bind:fields />

          <hr />
          <p class="txt-bold">Unique constraints and indexes ({indexes.length})</p>
          <div class="indexes-list">
            {#each indexes as idx}
              <button type="button" class="label handle success">
                {#if idx.startsWith("UNIQUE")}
                  <strong>Unique:</strong>
                {/if}
                <span class="txt">{idx.replace(/^UNIQUE\s+/, "")}</span>
              </button>
            {/each}
            <button type="button" class="label handle" onclick={() => indexes.push("")}>
              <i class="ri-add-line"></i>
              <span class="txt">New index</span>
            </button>
          </div>
        </div>
      </div>
    {:else}
      <div class="tab-content-wrapper" data-tab="API rules">
        <div class="collection-tab-content collection-rules-tab-content">
          <div class="grid-sm">
            <div class="col-sm-12">
              <div class="flex txt-hint txt-sm">
                <span class="txt">All rules follow the <a target="_blank" rel="noopener noreferrer" href="https://pocketbase.io/docs/api-rules-and-filters/">PocketBase filter syntax and operators</a>.</span>
                <strong tabindex="-1" class="m-l-auto link-hint" onclick={() => showRulesInfo = !showRulesInfo}>
                  {showRulesInfo ? "Hide available fields" : "Show available fields"}
                </strong>
              </div>
            </div>

            {#if showRulesInfo}
              <div class="col-sm-12">
                <div class="alert warning m-t-sm">
                  <div class="content">
                    <p>The following record fields are available:</p>
                    <div class="flex flex-wrap gap-5">
                      {#each fields as field}
                        <code>{field.name}</code>
                      {/each}
                    </div>
                    <hr class="m-t-10 m-b-10" />
                    <p>The request fields could be accessed with the special <strong>@request</strong> fields:</p>
                    <div class="flex flex-wrap gap-5">
                      <code>@request.headers.*</code>
                      <code>@request.query.*</code>
                      <code>@request.body.*</code>
                      <code>@request.auth.*</code>
                    </div>
                    <hr class="m-t-10 m-b-10" />
                    <p>You could also add constraints and query other collections using the <strong>@collection</strong> field:</p>
                    <div class="flex flex-wrap gap-5">
                      <code>@collection.ANY_COLLECTION_NAME.*</code>
                    </div>
                    <hr class="m-t-10 m-b-10" />
                    <p>Example rule:</p>
                    <code>@request.auth.id != ""</code>
                  </div>
                </div>
              </div>
            {/if}

            <div class="col-sm-12">
              <RuleField label="List/Search rule" name="listRule" bind:value={listRule} />
            </div>
            <div class="col-sm-12">
              <RuleField label="View rule" name="viewRule" bind:value={viewRule} />
            </div>
            <div class="col-sm-12">
              <RuleField label="Create rule" name="createRule" bind:value={createRule} />
            </div>
            <div class="col-sm-12">
              <RuleField label="Update rule" name="updateRule" bind:value={updateRule} />
            </div>
            <div class="col-sm-12">
              <RuleField label="Delete rule" name="deleteRule" bind:value={deleteRule} />
            </div>
          </div>
        </div>
      </div>
  {/if}
</div>

  {#if error}
    <div class="error-bar">{error}</div>
  {/if}

  <!-- Footer -->
  <footer class="modal-footer">
    <button type="button" class="btn transparent m-r-auto" onclick={() => show = false}>
      <span class="txt">Close</span>
    </button>
    {#if error}
      <i class="ri-error-warning-line txt-danger" title={error}></i>
    {/if}
    <div class="btns">
      <button
        type="button"
        class="btn expanded-lg"
        class:loading={saving}
        disabled={!name.trim() || saving}
        onclick={handleSave}
      >
        <span class="txt">Create</span>
      </button>
    </div>
  </footer>
</SlideOver>

<style>
  .modal-header.isolated {
    padding: var(--spacing);
    padding-top: calc(var(--spacing) - 10px);
    padding-bottom: calc(var(--spacing) - 10px);
    background: var(--color-base-200);
    border-bottom: 1px solid var(--color-base-300);
    flex-shrink: 0;
  }
  .grid.sm {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
  }
  .col-12 {
    width: 100%;
  }
  .flex {
    display: flex;
    align-items: center;
  }
  .flex-fill {
    flex: 1;
  }
  .modal-title {
    font-size: var(--font-size-base);
    font-weight: 600;
    align-content: center;
    margin: 0;
  }
  .fields {
    display: flex;
    width: 100%;
    align-items: stretch;
  }
  .field {
    position: relative;
    display: block;
    outline: 0;
    width: 100%;
    min-width: 0;
    border-radius: var(--radius-field);
    background: var(--input-color);
  }
  .field:not(.addon) > :first-child {
    border-top-left-radius: inherit;
    border-top-right-radius: inherit;
  }
  .field:not(.addon) > :last-child {
    border-bottom-left-radius: inherit;
    border-bottom-right-radius: inherit;
  }
  .field + .field,
  .fields .field:not(:first-child) {
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
  }
  .fields .field:not(:last-child) {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .field label {
    display: flex;
    width: 100%;
    gap: 5px;
    line-height: 1;
    align-items: center;
    align-self: center;
    min-height: 24px;
    padding: 9px var(--input-padding) 1px;
    font-weight: bold;
    white-space: normal;
    color: var(--color-base-hint);
    font-size: var(--font-size-sm);
  }
  .fields .addon {
    display: inline-flex;
    align-items: center;
    width: auto;
    flex-shrink: 0;
    color: var(--color-base-hint);
    position: relative;
  }
  .field.addon .collection-type-select {
    width: 100%;
  }
  .collection-type-select {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: space-between;
    white-space: nowrap;
  }
  .collection-type-select.outline {
    border: 2px solid currentColor;
    background: transparent;
    color: var(--color-base-content);
  }
  .collection-type-select.outline:hover {
    background: var(--color-base-200);
  }
  .m-l-auto {
    margin-left: auto;
  }
  :global(.collection-type-dropdown.open) {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    width: auto !important;
    min-width: 180px !important;
    z-index: 100;
    animation: dropDownFadeIn 0.12s ease-out;
  }
  :global(.collection-type-dropdown.open) :global(.dropdown-item) {
    white-space: nowrap;
  }
  .collection-type-dropdown .dropdown-item i {
    font-size: 16px;
  }
  .modal-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--spacing);
    scrollbar-width: thin;
  }
  .tab-content-wrapper {
    display: block;
  }
  .collection-tab-content {
    display: block;
  }
  .collection-fields-list {
    margin-bottom: 4px;
  }
  .empty-fields {
    text-align: center;
    padding: 24px 0;
    opacity: 0.4;
    font-size: var(--font-size-sm);
  }
  .txt-bold {
    font-size: var(--font-size-sm);
    font-weight: 600;
    margin-bottom: 8px;
    margin-top: 12px;
  }
  .indexes-list {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .indexes-list .label {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border-radius: 4px;
    font-size: var(--font-size-xs);
    border: 1px solid var(--color-base-400);
    background: var(--color-base-200);
    cursor: pointer;
  }
  .indexes-list .label strong {
    font-weight: 600;
  }
  .modal-footer {
    display: flex;
    align-items: center;
    width: 100%;
    gap: var(--spacing-sm);
    padding: var(--spacing);
    padding-top: calc(var(--spacing) - 10px);
    padding-bottom: calc(var(--spacing) - 10px);
    margin-top: auto;
    border-top: 1px solid var(--color-base-300);
    flex-shrink: 0;
  }
  .m-r-auto {
    margin-right: auto;
  }
  .txt-danger {
    color: var(--color-error);
    font-size: 16px;
  }
  .btns {
    display: inline-flex;
    align-items: stretch;
    min-width: 0;
  }
  .error-bar {
    padding: 8px 12px;
    font-size: var(--font-size-sm);
    color: var(--color-error);
    background: color-mix(in srgb, var(--color-error), var(--color-base-100) 80%);
    border-left: 3px solid var(--color-error);
    margin: 0 var(--spacing);
  }
  hr {
    border: none;
    border-top: 1px solid var(--color-base-300);
    margin: 12px 0;
  }
  .field .input {
    display: inline-block;
    vertical-align: top;
    outline: 0;
    border: 0;
    margin: 0;
    width: 100%;
    background: none;
    font-weight: normal;
    line-height: 1;
    padding: 10px var(--input-padding);
    color: var(--color-base-content);
    font-size: var(--font-size-base);
    font-family: var(--font-sans);
    align-self: stretch;
  }
  .field input::placeholder {
    user-select: none;
    color: var(--color-base-disabled);
    font-weight: inherit;
    font-family: inherit;
  }
  @keyframes dropDownFadeIn {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>
