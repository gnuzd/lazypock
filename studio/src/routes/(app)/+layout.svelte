<script lang="ts">
	import AppHeader from '$lib/components/AppHeader.svelte';

	let { children } = $props();
</script>

<div class="flex h-full flex-col">
	<AppHeader />
	{@render children()}
</div>
