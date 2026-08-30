<script lang="ts">
  import "../app.css";
  import { page } from "$app/state";
  import { nav, type NavItem } from "$lib/nav";
  import { slide } from "svelte/transition";

  let { children } = $props();
  let sidebarOpen = $state(false);
  let collapsed = $state<string>();

  function isActive(item: NavItem): boolean {
    const [base, hash] = item.href.split("#");
    if (item.children) {
      // Section parent: active while on its page (any anchor).
      return page.url.pathname === base;
    }
    if (hash) {
      // Anchor child: active only when on the page at that anchor.
      return page.url.pathname === base && page.url.hash === `#${hash}`;
    }
    // Plain link: exact pathname match.
    return page.url.pathname === base;
  }
</script>

<div class="min-h-screen flex flex-col bg-base-100 text-base-content">
  <!-- Navbar -->
  <header
    class="sticky top-0 z-40 flex items-center justify-between gap-3 border-b border-base-300 bg-base-100/90 backdrop-blur px-4 py-3 lg:px-6"
  >
    <div class="flex items-center gap-3">
      <button
        class="lg:hidden inline-flex items-center justify-center rounded-field w-9 h-9 border border-base-300"
        aria-label="Toggle menu"
        onclick={() => (sidebarOpen = !sidebarOpen)}
      >
        <svg
          xmlns="http://www.w3.org/2000/svg"
          class="w-5 h-5"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M4 6h16M4 12h16M4 18h16"
          />
        </svg>
      </button>
      <a href="/" class="flex items-center gap-2 font-semibold text-lg">
        <span
          class="inline-flex h-7 w-7 items-center justify-center rounded-box bg-primary text-primary-content text-sm font-bold"
          >L</span
        >
        <span>Lazypock</span>
      </a>
      <span
        class="hidden sm:inline-block rounded-field bg-base-200 px-2 py-0.5 text-xs text-base-content/70"
        >server + SDKs</span
      >
    </div>

    <nav class="hidden md:flex items-center gap-4 text-sm">
      <a class="hover:text-primary" href="/server">Server</a>
      <a class="hover:text-primary" href="/sdk">SDKs</a>
      <a
        class="rounded-field bg-neutral text-neutral-content px-3 py-1.5 font-medium hover:opacity-90"
        href="https://github.com/gnuzd/lazypock"
        target="_blank"
        rel="noreferrer">GitHub</a
      >
    </nav>
  </header>

  <div class="flex-1 flex mx-auto w-full max-w-[1400px]">
    <!-- Sidebar -->
    <aside
      class="fixed inset-y-0 left-0 top-[57px] z-30 w-72 shrink-0 overflow-y-auto border-r border-base-300 bg-base-100 px-4 py-6 transition-transform duration-200 lg:sticky lg:top-[57px] lg:h-[calc(100vh-57px)] lg:translate-x-0
			{sidebarOpen ? 'translate-x-0' : '-translate-x-full'}"
    >
      {#each nav as section}
        <div class="mb-5">
          <div
            class="flex w-full items-center justify-between gap-1 rounded-field px-2 py-1 text-xs font-semibold uppercase tracking-wide text-base-content/50"
          >
            <span>{section.title}</span>
          </div>
          <ul class="mt-1 space-y-0.5">
            {#each section.items as item}
              <li>
                <a
                  href={item.href}
                  class="block rounded-field px-2 py-1.5 text-sm text-base-content/80 hover:bg-base-200 hover:text-primary {isActive(
                    item,
                  )
                    ? 'bg-base-200 text-primary font-medium'
                    : ''}"
                  onclick={() => {
                    collapsed = item.label;
                    sidebarOpen = false;
                  }}
                >
                  {item.label}
                  {#if item.badge}
                    <span
                      class="ml-1.5 rounded-field bg-warning/10 text-warning px-1.5 py-0.5 text-[10px] font-medium align-middle"
                    >
                      {item.badge}
                    </span>
                  {/if}
                </a>
                {#if item.children && collapsed === item.label}
                  <ul
                    transition:slide
                    class="mt-0.5 ml-3 space-y-0.5 border-l border-base-300 pl-2"
                  >
                    {#each item.children as child}
                      <li>
                        <a
                          href={child.href}
                          class="block rounded-field px-2 py-1 text-[13px] text-base-content/60 hover:bg-base-200 hover:text-primary {isActive(
                            child,
                          )
                            ? 'text-primary font-medium'
                            : ''}"
                          onclick={() => (sidebarOpen = false)}
                        >
                          {child.label}
                        </a>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    </aside>

    {#if sidebarOpen}
      <button
        class="fixed inset-0 top-[57px] z-20 bg-neutral/40 lg:hidden"
        aria-label="Đóng menu"
        onclick={() => (sidebarOpen = false)}
      ></button>
    {/if}

    <!-- Content -->
    <main class="min-w-0 flex-1 px-5 py-8 lg:px-12">
      {@render children()}
    </main>
  </div>
</div>
