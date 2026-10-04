<script lang="ts">
	import '$lib/styles/app.css';
	import { Toaster } from 'svelte-sonner';

	import favicon from '$lib/assets/favicon.svg';
	import { onMount } from 'svelte';
	import AppDialog from '$lib/components/AppDialog.svelte';
	import { client } from '$lib/client';
	import { base, resolve } from '$app/paths';
	import { browser } from '$app/environment';
	import { goto } from '$app/navigation';

	let { children } = $props();
	let initialized = $state(false);
	onMount(async () => {
		try {
			const theme = localStorage.getItem('lazypock-theme');
			if (theme) document.documentElement.setAttribute('data-theme', theme);

			if (!browser) return;

			const path = window.location.pathname;
			const isLoginPage = path === base + '/login';

			if (client.authStore.isValid && !isLoginPage) {
				// Verify the token is actually valid before redirecting
				try {
					await client.me();

					if (
						path.startsWith(base + '/collections') ||
						path.startsWith(base + '/logs') ||
						path.startsWith(base + '/settings')
					) {
						return;
					}
					goto(resolve('/collections?collection=users'));
					return;
				} catch {
					// Token is stale — clear it and show login
					client.authStore.clear();
					if (!isLoginPage) {
						goto(resolve('/login'));
						return;
					}
					// Already on login page — let it render
				}
			} else if (!isLoginPage) {
				goto(resolve('/login'));
				return;
			}
		} finally {
			initialized = true;
		}
	});
</script>

<svelte:head><link rel="icon" href={favicon} /></svelte:head>
{#if initialized}
	{@render children()}
{/if}
<Toaster richColors position="top-right" />

<!-- Single app-wide confirm / alert / prompt dialog (native window dialogs are
     never used in the Studio). -->
<AppDialog />
