<script lang="ts">
	import { SvelteFlowProvider } from '@xyflow/svelte';
	import '@xyflow/svelte/dist/style.css';
	import ErdFlow from './ErdFlow.svelte';

	/**
	 * ERD ("Fields and relations") view — PocketBase's Collections overview
	 * tab, built on Svelte Flow. Each collection is a node (name + type +
	 * fields, relation fields highlighted), relations are rendered as edges
	 * from the relation field's row to the target collection. Zoom / pan /
	 * drag via the Svelte Flow controls; clicking a collection highlights its
	 * relations and dims the rest.
	 */
	let {
		collections = []
	}: {
		collections?: Record<string, unknown>[];
	} = $props();
</script>

<div class="absolute inset-0 overflow-hidden rounded-field border border-base-300">
	{#if collections.length === 0}
		<div class="flex h-full items-center justify-center text-sm text-base-content/50">
			No collections to show.
		</div>
	{:else}
		<SvelteFlowProvider>
			<ErdFlow {collections} />
		</SvelteFlowProvider>
	{/if}
</div>
