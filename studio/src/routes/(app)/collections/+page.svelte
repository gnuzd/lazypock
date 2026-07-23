<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { Folder } from '@lucide/svelte';
	import { setSidebar } from '$lib/sidebar.svelte';

	let collections = $state<Record<string, unknown>[]>([]);
	let activeName = $state('');
	let search = $state('');

	let filtered = $derived.by(() => {
		if (!search) return collections;
		const q = search.toLowerCase();
		return collections.filter((c) => (c.name as string)?.toLowerCase().includes(q));
	});

	onMount(async () => {
		try {
			const result = await client.listCollections('page=1&perPage=200');
			collections = result?.items || [];
			const params = new URLSearchParams(window.location.search);
			activeName = params.get('collection') ?? (collections?.[0]?.name as string | undefined) ?? '';
		} catch {
			// ignore
		}
	});

	function selectCollection(name: string) {
		activeName = name;
		history.replaceState(null, '', '?collection=' + encodeURIComponent(name));
	}

	function newCollection() {
		// stub
	}

	setSidebar(headerContent, bodyContent, footerContent);
</script>

{#snippet headerContent()}
	<input
		type="text"
		class="input input-sm w-full"
		placeholder="Search..."
		bind:value={search}
	/>
{/snippet}

{#snippet bodyContent()}
	{#if filtered.length === 0}
		<div class="p-4 text-center opacity-40 text-sm">No collections</div>
	{:else}
		{#each filtered as coll (coll.id)}
			<button
				class="flex items-center gap-2 w-[calc(100%-12px)] mx-1.5 px-3 py-1.5 border-none rounded-field text-sm text-base-content cursor-pointer text-left transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
				class:bg-base-200={coll.name === activeName}
				class:font-medium={coll.name === activeName}
				onclick={() => selectCollection(coll.name as string)}
			>
				<Folder class="w-4 h-4 opacity-60 shrink-0" />
				<span class="truncate">{coll.name as string}</span>
				<span class="ml-auto text-xs opacity-40">{(coll.schema as unknown[])?.length ?? 0}</span>
			</button>
		{/each}
	{/if}
{/snippet}

{#snippet footerContent()}
	<button class="btn btn-primary btn-full" onclick={newCollection}>+ New Collection</button>
{/snippet}
