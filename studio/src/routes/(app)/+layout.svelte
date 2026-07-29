<script lang="ts">
	import AppHeader from '$lib/components/AppHeader.svelte';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import { getSidebar } from '$lib/sidebar.svelte';

	let { children } = $props();

	const sidebar = getSidebar();

	$effect(() => {
		// If sidebar has no content, hide it via CSS
	});
</script>

<div class="flex h-full flex-col">
	<AppHeader />
	<div class="flex flex-1 overflow-hidden">
		{#if sidebar.header || sidebar.body || sidebar.footer}
			<Sidebar header={sidebar.header} body={sidebar.body} footer={sidebar.footer} />
		{/if}
		<main class="flex-1 overflow-auto p-6">
			{@render children()}
		</main>
	</div>
</div>
