<script>
  let { show = $bindable(false), children } = $props();

  function close() {
    show = false;
  }
</script>

<svelte:body
  onkeydown={(e) => { if (e.key === "Escape" && show) close(); }}
/>

{#if show}
  <div class="modal collection-upsert-modal open" role="dialog" onclick={(e) => e.stopPropagation()} onkeydown={(e) => { if (e.key === 'Escape') close(); }}>
    {@render children?.()}
  </div>
{/if}

<style>
  .modal {
    position: fixed;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    width: 720px;
    max-width: 100%;
    height: 100%;
    border: 0;
    outline: 0;
    margin: 0;
    top: 0;
    right: 0;
    word-break: break-word;
    color: var(--color-base-content);
    background: var(--color-base-100);
    box-shadow: -1px 0px 5px 0 rgba(34, 36, 36, 0.1);
    overscroll-behavior: none;
  }

  /* Overlay via ::before */
  .modal::before {
    content: "";
    display: block;
    position: fixed;
    z-index: -2;
    left: -100vw;
    top: -100vh;
    width: 200vw;
    height: 200vh;
    background: var(--modal-overlay);
  }
  .modal::after {
    content: "";
    display: block;
    position: absolute;
    z-index: -1;
    left: 0;
    top: 0;
    width: 100%;
    height: 100%;
    background: inherit;
  }
</style>
