<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { base } from '$app/paths';
	import { Folder, Plus } from '@lucide/svelte';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import Button from '$lib/components/Button.svelte';
	import CollectionEditor from '$lib/components/CollectionEditor.svelte';
	import SidePane from '$lib/components/SidePane.svelte';
	import {
		activeName,
		collections,
		loadCollections,
		subscribeToCollectionChanges
	} from '$lib/collectionsStore';

	let { children } = $props();

	let search = $state('');
	let showSystem = $state(false);

	const filtered = $derived.by(() => {
		const list = $collections;
		if (!search) return list;
		const q = search.toLowerCase();
		return list.filter((c) => (c.name as string)?.toLowerCase().includes(q));
	});

	onMount(() => {
		loadCollections();
		return subscribeToCollectionChanges();
	});

	function select(name: string) {
		$activeName = name;
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		goto(base + '/collections?collection=' + encodeURIComponent(name), {
			replaceState: true,
			keepFocus: true,
			noScroll: true
		});
	}

	// ── New-Collection side pane state ──
	let showNewPane = $state(false);
</script>

{#snippet headerContent()}
	<input type="text" class="input input-sm w-full" placeholder="Search..." bind:value={search} />
{/snippet}

{#snippet bodyContent()}
	{#if filtered.length === 0}
		<div class="p-4 text-center text-sm opacity-40">No collections</div>
	{:else}
		{@const userCollections = filtered.filter((c) => !c.system)}
		{@const systemCollections = filtered.filter((c) => c.system)}

		{#if userCollections.length > 0}
			{#each userCollections as coll (coll.id)}
				<div
					class="mx-1.5 flex w-[calc(100%-12px)] cursor-pointer items-center gap-2 rounded-field border-none px-3 py-1.5 text-left text-sm text-base-content transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
					class:bg-base-200={coll.name === $activeName}
					class:font-medium={coll.name === $activeName}
					role="button"
					tabindex="0"
					onclick={() => select(coll.name as string)}
					onkeydown={(e) => {
						if (e.key === 'Enter') select(coll.name as string);
					}}
				>
					<Folder class="h-4 w-4 shrink-0 opacity-60" />
					<span class="flex-1 truncate">{coll.name as string}</span>
					{#if (coll.type as string) === 'view'}
						<span class="rounded bg-info/20 px-1 py-0.5 text-[10px] font-medium text-info"
							>view</span
						>
					{/if}
					<span class="mr-1 text-xs opacity-40">{(coll.schema as unknown[])?.length ?? 0}</span>
				</div>
			{/each}
		{/if}

		{#if systemCollections.length > 0}
			<div
				class="mx-1.5 flex w-[calc(100%-12px)] cursor-pointer items-center gap-2 px-3 py-1.5 text-xs font-medium text-base-content/50 select-none"
				onclick={() => (showSystem = !showSystem)}
				role="button"
				tabindex="0"
				onkeydown={(e) => {
					if (e.key === 'Enter') showSystem = !showSystem;
				}}
			>
				<svg
					class="h-3 w-3 transition-transform duration-150"
					class:rotate-90={showSystem}
					fill="none"
					viewBox="0 0 24 24"
					stroke="currentColor"
					stroke-width="2"><polyline points="9 18 15 12 9 6" /></svg
				>
				<span>System ({systemCollections.length})</span>
			</div>
			{#if showSystem}
				{#each systemCollections as coll (coll.id)}
					<div
						class="mx-1.5 flex w-[calc(100%-12px)] cursor-pointer items-center gap-2 rounded-field border-none px-3 py-1.5 text-left text-sm text-base-content transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
						class:bg-base-200={coll.name === $activeName}
						class:font-medium={coll.name === $activeName}
						role="button"
						tabindex="0"
						onclick={() => select(coll.name as string)}
						onkeydown={(e) => {
							if (e.key === 'Enter') select(coll.name as string);
						}}
					>
						<svg
							class="h-4 w-4 shrink-0"
							fill="none"
							viewBox="0 0 24 24"
							stroke="currentColor"
							stroke-width="2"
							style="color:var(--color-warning)"
							><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" /></svg
						>
						<span class="flex-1 truncate">{coll.name as string}</span>
						<span class="rounded bg-warning/20 px-1 py-0.5 text-[10px] font-medium text-warning"
							>system</span
						>
					</div>
				{/each}
			{/if}
		{/if}
	{/if}
{/snippet}

{#snippet footerContent()}
	<Button
		class="btn-primary btn-full"
		onclick={() => {
			showNewPane = true;
		}}><Plus size={18} /> New Collection</Button
	>
{/snippet}

<!-- Layout: sidebar left, main content right -->
<div class="flex flex-1 overflow-hidden">
	<Sidebar header={headerContent} body={bodyContent} footer={footerContent} />
	<main class="flex-1 overflow-auto p-6">
		{@render children()}
	</main>
</div>

<!-- New collection SidePane (right-side pane instead of a full page) -->
<SidePane
	bind:show={showNewPane}
	title="New Collection"
	closable={false}
	onCloseRequest={() => false}
>
	<CollectionEditor
		editingCollectionId={null}
		existingName=""
		onClose={() => {
			showNewPane = false;
		}}
	/>
</SidePane>
