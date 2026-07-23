<script>
  import { store } from "../lib/store.svelte.js";
  import { api } from "../lib/api.js";

  function logout() {
    api.logout();
    store.superuser = null;
  }
</script>

<header class="app-header">
  <span class="app-header-brand">Lazypock</span>
  <nav class="app-header-nav">
    <a href="/_/" class="active">Collections</a>
    <a href="/_/logs">Logs</a>
    <a href="/_/settings">Settings</a>
  </nav>
  <div class="app-header-actions">
    <button class="btn btn-ghost btn-sm" onclick={logout}>Logout</button>
  </div>
</header>

<style>
  .app-header {
    display: flex;
    align-items: center;
    height: 44px;
    padding: 0 var(--spacing-sm);
    background: var(--color-primary);
    color: var(--color-primary-content);
    flex-shrink: 0;
  }
  .app-header-brand {
    font-size: var(--font-size-base);
    font-weight: 600;
    margin-right: var(--spacing);
    padding-right: var(--spacing-sm);
    border-right: 1px solid color-mix(in srgb, var(--color-primary-content) 20%, transparent);
  }
  .app-header-nav {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .app-header-nav :global(a) {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 12px;
    border-radius: var(--radius-field);
    font-size: var(--font-size-sm);
    font-weight: 500;
    transition: background var(--animation-speed-fast);
  }
  .app-header-nav :global(a:hover) {
    background: color-mix(in srgb, var(--color-primary-content) 10%, transparent);
  }
  .app-header-actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: var(--font-size-sm);
  }
</style>
