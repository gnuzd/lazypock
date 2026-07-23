<script>
  let { label, value = $bindable(null), name = "", placeholder = "Leave empty to grant everyone access...", disabled = false } = $props();

  let prevValue = $state("");
  let focused = $state(false);

  function updateValue(newVal) {
    value = newVal;
  }

  function lock() {
    if (value === null) return;
    prevValue = value;
    value = null;
  }

  function unlock() {
    if (prevValue != null) {
      value = prevValue;
    } else {
      value = "";
    }
    // focus the input after unlock
    requestAnimationFrame(() => {
      const input = document.getElementById("rule-" + name);
      input?.focus();
    });
  }
</script>

<div class="field rule-field" class:locked={value === null} class:disabled>
  <label for={"rule-" + name}>
    {#if typeof label === "string"}
      <span class="txt">{label}</span>
    {:else if label}
      {@render label()}
    {:else}
      <span class="txt">Rule</span>
    {/if}
    <span class="txt superusers-label" class:hidden={value !== null}>(Superusers only)</span>
  </label>

  {#if value === null}
    <!-- locked state: overlay to unlock -->
    <button type="button" class="unlock-overlay" {disabled} onclick={unlock}>
      <span class="txt">Unlock and set custom rule</span>
      <i class="ri-lock-unlock-line"></i>
    </button>
  {:else}
    <!-- unlocked state: input + lock button -->
    <div class="flex rule-input-row">
      <div class="code-editor-wrapper" class:focused>
        <input
          id={"rule-" + name}
          type="text"
          class="input code-editor"
          placeholder={placeholder}
          bind:value
          onfocus={() => focused = true}
          onblur={() => focused = false}
          {disabled}
        />
      </div>
      <button
        type="button"
        class="superuser-toggle"
        hidden={false}
        {disabled}
        onclick={lock}
      >
        <i class="ri-lock-line"></i>
        <span class="txt">Set superusers only</span>
      </button>
    </div>
  {/if}

  {#if !disabled}
    <i class="ri-information-line link-hint info-icon"
      hidden={value !== null}
      title="The main record fields hold the values that are going to be inserted in the database.">
    </i>
  {/if}
</div>

<style>
  .rule-field {
    min-height: 67px;
    position: relative;
  }
  .rule-field label {
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
    position: relative;
    z-index: 2;
  }
  .superusers-label {
    opacity: 1;
    visibility: visible;
    transform: translateX(0);
    transition: opacity var(--animation-speed, 0.12s), visibility var(--animation-speed, 0.12s), transform var(--animation-speed, 0.12s);
  }
  .superusers-label.hidden,
  .superusers-label[hidden] {
    display: inline !important;
    opacity: 0;
    visibility: hidden;
    transform: translateX(3px);
  }

  .rule-input-row {
    display: flex;
    align-items: stretch;
  }

  .code-editor-wrapper {
    flex: 1;
    min-width: 0;
  }
  .code-editor-wrapper .code-editor {
    display: block;
    width: 100%;
    outline: 0;
    border: 0;
    margin: 0;
    background: none;
    font-weight: normal;
    line-height: 1;
    padding: 10px var(--input-padding);
    color: var(--color-base-content);
    font-size: var(--font-size-base);
    font-family: var(--font-mono);
    align-self: stretch;
    min-height: 41px;
  }
  .code-editor-wrapper .code-editor::placeholder {
    user-select: none;
    color: var(--color-base-disabled);
    font-weight: inherit;
    font-family: var(--font-sans);
  }

  .superuser-toggle,
  .superuser-toggle:last-child:not(.btn) {
    border-radius: 0;
    border-bottom-left-radius: var(--radius-field);
    border-top-right-radius: var(--radius-field);
  }
  .superuser-toggle {
    position: relative;
    z-index: 3;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    outline: 0;
    cursor: pointer;
    user-select: none;
    font-weight: bold;
    line-height: 1;
    font-size: var(--font-size-sm);
    padding: 8px 10px;
    color: var(--color-base-hint);
    background: var(--input-focus-color);
    border: 1px solid var(--color-base-400);
    border-top: 0;
    border-right: 0;
    transition: color var(--animation-speed, 0.12s), background var(--animation-speed, 0.12s);
  }
  .superuser-toggle:hover,
  .superuser-toggle:active,
  .superuser-toggle:focus-visible {
    color: var(--color-success);
    background: var(--color-base-300);
  }
  .superuser-toggle[disabled] {
    cursor: not-allowed;
    color: var(--color-base-disabled);
    background: var(--color-base-200);
  }

  .unlock-overlay {
    position: absolute;
    z-index: 1;
    left: 0;
    top: 0;
    display: flex;
    gap: 10px;
    align-items: center;
    justify-content: flex-end;
    width: 100%;
    height: 100%;
    cursor: pointer;
    user-select: none;
    font-weight: bold;
    font-size: var(--font-size-sm);
    color: var(--color-success);
    padding: 5px var(--spacing);
    border-radius: var(--radius-field);
    background: var(--color-base-200);
    border: 2px solid var(--color-base-300);
    transition: border-color var(--animation-speed, 0.12s);
  }
  .unlock-overlay .txt {
    opacity: 0;
    transform: translateX(3px);
    transition: opacity var(--animation-speed, 0.12s), transform var(--animation-speed, 0.12s);
  }
  .unlock-overlay i {
    font-size: 1.4em;
  }
  .unlock-overlay:hover,
  .unlock-overlay:active,
  .unlock-overlay:focus-visible {
    border-color: var(--color-base-400);
  }
  .unlock-overlay:hover .txt,
  .unlock-overlay:active .txt,
  .unlock-overlay:focus-visible .txt {
    opacity: 1;
    transform: translateX(0);
  }
  .unlock-overlay:active {
    border-color: var(--color-base-500);
    transition-duration: var(--active-animation-speed, 0.05s);
  }
  .unlock-overlay[disabled] {
    cursor: not-allowed;
  }
  .unlock-overlay[disabled] .txt {
    display: none;
  }

  .rule-field.locked label {
    pointer-events: none;
  }

  .info-icon {
    position: absolute;
    right: 8px;
    bottom: 8px;
    font-size: 14px;
    color: var(--color-base-hint);
  }
</style>
