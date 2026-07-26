<script lang="ts">
	import Button from '$lib/components/Button.svelte';
	import { client, disconnectRealtime } from '$lib/client';
	import { base } from '$app/paths';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';

	function logout() {
		disconnectRealtime();
		client.logout();
		window.location.href = base + '/login';
	}

	function navClass(path: string): string {
		const active = page.url.pathname.endsWith(path);
		return (
			'text-sm font-medium btn-sm ' +
			(active ? 'bg-primary-content/10' : 'hover:bg-primary-content/10')
		);
	}

	function _goto(path: string) {
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		goto(base + path);
	}
</script>

<header class="flex h-11 shrink-0 items-center gap-2 bg-primary px-4 text-primary-content">
	<span class=" mr-3 border-r border-primary-content/20 pr-3 font-semibold"> Lazypock </span>
	<nav class="flex items-center gap-1">
		<Button class={navClass('/collections')} onclick={() => _goto('/collections')}
			>Collections</Button
		>
		<Button class={navClass('/logs')} onclick={() => _goto('/logs')}>Logs</Button>
		<Button class={navClass('/settings')} onclick={() => _goto('/settings')}>Settings</Button>
	</nav>
	<div class="ml-auto">
		<Button class="btn-sm text-sm font-medium hover:bg-primary-content/10" onclick={logout}
			>Logout</Button
		>
	</div>
</header>
