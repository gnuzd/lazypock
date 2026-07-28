<script lang="ts">
	import '$lib/styles/app.css';
	import { Toaster } from 'svelte-sonner';

	import favicon from '$lib/assets/favicon.svg';
	import { onMount } from 'svelte';
	import { client, connectRealtime } from '$lib/client';
	import { base } from '$app/paths';
	import { browser } from '$app/environment';

	let { children } = $props();
	let initialized = $state(false);
	onMount(async () => {
		try {
			await client.authStore.init();

			if (!browser) return;

			const path = window.location.pathname;
			const isLoginPage = path === base + '/login';

			if (client.authStore.isValid && !isLoginPage) {
				// Verify the token is actually valid before redirecting
				try {
					await client.me();
					// Token is good — connect realtime and stay
					connectRealtime();

					if (
						path.startsWith(base + '/collections') ||
						path.startsWith(base + '/logs') ||
						path.startsWith(base + '/settings')
					) {
						return;
					}
					window.location.href = base + '/collections?collection=users';
					return;
				} catch {
					// Token is stale — clear it and show login
					client.authStore.clear();
					if (!isLoginPage) {
						window.location.href = base + '/login';
						return;
					}
					// Already on login page — let it render
				}
			} else if (!isLoginPage) {
				window.location.href = base + '/login';
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
