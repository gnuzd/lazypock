<script>
  import { addableFieldTypes, fieldTypes, createField } from "../lib/fieldTypes.js";

  let { fields = $bindable([]) } = $props();
  let open = $state(false);
  let wrapper;

  $effect(() => {
    if (open) {
      function handle(e) {
        if (wrapper && !wrapper.contains(e.target)) {
          open = false;
        }
      }
      document.addEventListener("mousedown", handle);
      return () => document.removeEventListener("mousedown", handle);
    }
  });

  function addField(type) {
    const newField = createField(type, fields);
    const idx = fields.findLastIndex((f) => f.type !== "autodate");
    if (type !== "autodate" && idx >= 0) {
      fields.splice(idx + 1, 0, newField);
    } else {
      fields.push(newField);
    }
    open = false;
  }

  function toggle(e) {
    e.preventDefault();
    open = !open;
  }
</script>

<div bind:this={wrapper} class="new-collection-field-btn-wrapper">
  <button
    type="button"
    class="btn block outline new-field-btn"
    onclick={toggle}
  >
    <i class="ri-add-line"></i>
    <span class="txt">New field</span>
  </button>
  {#if open}
    <div class="dropdown open field-types-dropdown">
      {#each addableFieldTypes as type}
        <button
          type="button"
          class="dropdown-item"
          onmousedown={(e) => { e.preventDefault(); addField(type); }}
        >
          <i class={fieldTypes[type].icon}></i>
          <span class="txt">{fieldTypes[type].label}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .new-collection-field-btn-wrapper {
    margin: var(--spacing-sm) 0;
    position: relative;
  }
  .new-field-btn {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    flex-shrink: 0;
    cursor: pointer;
    min-height: var(--btn-height);
    padding: 5px 25px;
    font-weight: bold;
    font-size: var(--font-size-base);
    font-family: var(--font-sans);
    border-radius: var(--radius-field);
    border: 2px solid currentColor;
    background: transparent;
    color: var(--color-base-content);
    transition: background var(--animation-speed), color var(--animation-speed);
  }
  .new-field-btn:hover {
    background: var(--color-base-200);
  }
  .new-field-btn i {
    font-size: 1.25em;
  }
  .field-types-dropdown {
    display: flex;
    flex-direction: row;
    flex-wrap: wrap;
    gap: 0;
    padding: 5px;
    margin: 4px 0;
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    z-index: 100;
    min-width: 300px;
    background: var(--color-base-100);
    box-shadow: var(--shadow);
    border-radius: var(--radius-field);
    border: 2px solid var(--color-primary);
    animation: dropDownFadeIn var(--animation-speed-slow) ease-out;
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
  .field-types-dropdown .dropdown-item {
    width: 25%;
    padding: 10px;
    margin: 2px 0;
    outline: 0;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 8px;
    user-select: none;
    word-break: break-word;
    color: var(--color-base-content);
    border-radius: var(--radius-field);
    border: none;
    background: none;
    font-family: inherit;
    font-size: inherit;
    text-align: left;
  }
  .field-types-dropdown .dropdown-item:hover {
    background: var(--color-base-200);
  }
  @media (max-width: 600px) {
    .field-types-dropdown .dropdown-item {
      width: 50%;
    }
  }
</style>
