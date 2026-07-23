<script>
  import { onMount } from "svelte";
  import { store } from "./lib/store.svelte.js";
  import AppHeader from "./components/AppHeader.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import CollectionsPage from "./pages/Collections.svelte";
  import LoginPage from "./pages/Login.svelte";
  import NewCollectionModal from "./components/NewCollectionModal.svelte";

  let activeCollectionName = $state("");
  let showNewCollection = $state(false);
  let currentPage = $state("collections");
  let notFound = $state("");

  function getRoute() {
    const path = window.location.pathname.replace(/^\/_\//, "") || "";
    const params = new URLSearchParams(window.location.search);
    const page = path || "collections";

    if (page !== "collections" && page !== "logs" && page !== "settings") {
      notFound = page;
      return { page: "not-found", name: "" };
    }

    notFound = "";
    return { page, name: params.get("collection") || "" };
  }

  function navigate(path) {
    window.history.pushState({}, "", "/_/" + path);
    const route = getRoute();
    currentPage = route.page;
    if (route.name) activeCollectionName = route.name;
  }

  onMount(async () => {
    await store.init();

    window.addEventListener("popstate", () => {
      const route = getRoute();
      currentPage = route.page;
      if (route.name) activeCollectionName = route.name;
    });

    if (store.superuser) {
      await store.loadCollections();
      const route = getRoute();

      if (!route.name && store.collections.length > 0) {
        const first = store.collections[0];
        activeCollectionName = first.name;
        navigate("collections?collection=" + first.name);
      } else if (route.name) {
        activeCollectionName = route.name;
        currentPage = route.page;
      } else {
        currentPage = route.page;
      }
    }
  });

  function selectCollection(name) {
    activeCollectionName = name;
    navigate("collections?collection=" + name);
  }

  function handleLogin() {
    store.loadCollections().then(() => {
      if (store.collections.length > 0) {
        const first = store.collections[0];
        activeCollectionName = first.name;
        navigate("collections?collection=" + first.name);
      } else {
        navigate("collections");
      }
    });
  }
</script>

{#if !store.ready}
  <div class="loader-full">
    <span>Loading...</span>
  </div>
{:else if !store.superuser}
  <LoginPage onlogin={handleLogin} />
{:else if currentPage === "not-found"}
  <div class="app">
    <AppHeader />
    <div class="app-body">
      <main class="app-main">
        <div class="page">
          <div class="page-content" style="display:flex;align-items:center;justify-content:center;flex:1;flex-direction:column;gap:8px">
            <h2 style="font-size:48px;opacity:0.2;font-weight:700">404</h2>
            <p style="opacity:0.5">Collection "<strong>{notFound}</strong>" not found</p>
            <button class="btn btn-primary" onclick={() => { if (store.collections.length > 0) { selectCollection(store.collections[0].name); } else { navigate('collections'); } }}>
              Back to Collections
            </button>
          </div>
        </div>
      </main>
    </div>
  </div>
{:else}
  <div class="app">
    <AppHeader />
    <div class="app-body">
      <Sidebar
        collections={store.collections}
        activeName={activeCollectionName}
        onselect={selectCollection}
        onnew={() => showNewCollection = true}
      />
      <main class="app-main">
        {#if currentPage === "collections"}
          <CollectionsPage collectionName={activeCollectionName} on404={(id) => { notFound = id; currentPage = "not-found"; }} />
        {:else if currentPage === "logs"}
          <div class="page">
            <div class="page-header"><h3>Logs</h3></div>
            <div class="page-content"><div class="empty-state"><p>Coming soon</p></div></div>
          </div>
        {:else if currentPage === "settings"}
          <div class="page">
            <div class="page-header"><h3>Settings</h3></div>
            <div class="page-content"><div class="empty-state"><p>Coming soon</p></div></div>
          </div>
        {/if}
      </main>
    </div>
  </div>
{/if}

<NewCollectionModal bind:show={showNewCollection} />
<style>
  .loader-full {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 100vh;
    background: var(--color-base-100);
  }
  .app {
    display: flex;
    flex-direction: column;
    min-height: 100vh;
    background: var(--color-primary);
  }
  .app-body {
    display: flex;
    flex: 1;
    overflow: hidden;
    border-radius: var(--radius-box) var(--radius-box) 0 0;
    background: var(--color-base-100);
  }
  .app-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
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
</style>
