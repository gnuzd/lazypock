<script lang="ts">
	import { page } from '$app/state';
	import CollectionEditor from '$lib/components/CollectionEditor.svelte';

	const name = $derived(page.params.name ?? '');
</script>

{#if name}
	<CollectionEditor editingCollectionId={name} existingName={name} />
{/if}
