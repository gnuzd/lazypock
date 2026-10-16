<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import CollectionEditor from '$lib/components/CollectionEditor.svelte';

	const name = $derived(page.params.name ?? '');
</script>

{#if name}
	<CollectionEditor
		editingCollectionId={name}
		existingName={name}
		onDeleted={() => {
			// The collection this route is about is gone. Staying on `/<name>` would
			// keep a dead editor on screen and the URL naming something that does not
			// exist, which is the same stale-selection problem the list route had.
			void goto(resolve('/collections'), { replaceState: true });
		}}
	/>
{/if}
