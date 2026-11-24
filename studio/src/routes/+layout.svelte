<script lang="ts">
	import { Toaster } from 'svelte-sonner';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';

	import '#lib/styles/app.css';
	import favicon from '#lib/assets/favicon.svg';
	import store from '#lib/store.svelte.ts';
	import type { LayoutProps } from './$types';
	import { browser } from '$app/env';
	import { client } from '#lib/api/client.ts';
	import DrawerContainer from '#lib/components/ui/DrawerContainer.svelte';

	let { children }: LayoutProps = $props();

	onMount(async () => {
		if (await client.setup.required()) {
			goto('/setup');
		} else {
			if (!client.auth.record) {
				goto('/login');
			}
		}
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{@render children()}

<Toaster position="bottom-right" richColors />
<DrawerContainer />
