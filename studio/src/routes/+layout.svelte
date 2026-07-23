<script lang="ts">
	import '$lib/styles/app.css';
	import { Toaster } from 'svelte-sonner';

	import favicon from '$lib/assets/favicon.svg';
	import { onMount } from 'svelte';
	import { client } from '$lib/client';
	import { base } from '$app/paths';
	import { browser } from '$app/environment';

	let { children } = $props();

	onMount(async () => {
		await client.authStore.init();

		if (!browser) return;

		const path = window.location.pathname;
		const isLoginPage = path === base + '/login';

		if (client.authStore.isValid) {
			if (
				path.startsWith(base + '/collections') ||
				path.startsWith(base + '/logs') ||
				path.startsWith(base + '/settings')
			) {
				return;
			}
			try {
				const result = await client.listCollections('page=1&perPage=200');
				const name = result?.items?.[0]?.name ?? '';
				window.location.href = base + '/collections?collection=' + name;
			} catch {
				window.location.href = base + '/collections';
			}
		} else if (!isLoginPage) {
			window.location.href = base + '/login';
		}
	});
</script>

<svelte:head><link rel="icon" href={favicon} /></svelte:head>
{@render children()}
<Toaster richColors position="top-right" />
