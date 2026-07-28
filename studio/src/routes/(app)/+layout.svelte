<script lang="ts">
	import AppHeader from '$lib/components/AppHeader.svelte';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import { getSidebar } from '$lib/sidebar.svelte';

	let { children } = $props();

	const sidebar = getSidebar();
</script>

<div class="flex h-full flex-col">
	<AppHeader />
	<div class="flex flex-1 overflow-hidden">
		<Sidebar header={sidebar.header} body={sidebar.body} footer={sidebar.footer} />
		<main class="flex-1 overflow-auto p-6">
			{@render children()}
		</main>
	</div>
</div>
