<script lang="ts">
	import '../app.css';
	import { nav } from '$lib/nav';

	let { children } = $props();
	let sidebarOpen = $state(false);
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
				<svg xmlns="http://www.w3.org/2000/svg" class="w-5 h-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
					<path stroke-linecap="round" stroke-linejoin="round" d="M4 6h16M4 12h16M4 18h16" />
				</svg>
			</button>
			<a href="#overview" class="flex items-center gap-2 font-semibold text-lg">
				<span
					class="inline-flex h-7 w-7 items-center justify-center rounded-box bg-primary text-primary-content text-sm font-bold"
					>L</span
				>
				<span>lazypock<span class="text-primary">-ts</span></span>
			</a>
			<span class="hidden sm:inline-block rounded-field bg-base-200 px-2 py-0.5 text-xs text-base-content/70"
				>TypeScript SDK</span
			>
		</div>

		<nav class="hidden md:flex items-center gap-4 text-sm">
			<a class="hover:text-primary" href="#quick-start">Quick Start</a>
			<a class="hover:text-primary" href="#type-safety">Type Safety</a>
			<a class="hover:text-primary" href="#client">API</a>
			<a
				class="rounded-field bg-neutral text-neutral-content px-3 py-1.5 font-medium hover:opacity-90"
				href="https://github.com/gnuzd/lazypock-ts"
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
				<div class="mb-6">
					<p class="mb-2 px-2 text-xs font-semibold uppercase tracking-wide text-base-content/50">
						{section.title}
					</p>
					<ul class="space-y-0.5">
						{#each section.items as item}
							<li>
								<a
									href={item.href}
									class="block rounded-field px-2 py-1.5 text-sm text-base-content/80 hover:bg-base-200 hover:text-primary"
									onclick={() => (sidebarOpen = false)}
								>
									{item.label}
								</a>
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
