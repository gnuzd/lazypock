<script lang="ts">
	import { Toaster } from 'svelte-sonner';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';

	import '#lib/styles/app.css';
	import favicon from '#lib/assets/favicon.svg';
	import store from '#lib/store.svelte.ts';
	import type { LayoutProps } from './$types';
	import { browser } from '$app/env';

	let { children }: LayoutProps = $props();

	onMount(() => {
		if (!store.auth) {
			goto('/setup');
		}
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{@render children()}
{#if browser}
	<Toaster position="bottom-right" richColors />
{/if}
