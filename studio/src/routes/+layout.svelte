<script lang="ts">
	import '$lib/styles/app.css';
	import { Toaster } from 'svelte-sonner';

	import favicon from '$lib/assets/favicon.svg';
	import { onMount } from 'svelte';
	import { client } from '$lib/client';
	import { base } from '$app/paths';
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
					goto(base + '/collections?collection=users');
					return;
				} catch {
					// Token is stale — clear it and show login
					client.authStore.clear();
					if (!isLoginPage) {
						goto(base + '/login');
						return;
					}
					// Already on login page — let it render
				}
			} else if (!isLoginPage) {
				goto(base + '/login');
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
