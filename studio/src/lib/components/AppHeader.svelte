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
			'text-sm font-medium btn-sm' +
			(active
				? ' bg-primary-content/10'
				: ' hover:bg-primary-content/10')
		);
	}

  function _goto(path:string) {
    goto(base + path)
  }
</script>

<header class="flex items-center h-11 px-4 gap-2 bg-primary text-primary-content shrink-0">
	<span
		class=" font-semibold pr-3 mr-3 border-r border-primary-content/20"
	>
		Lazypock
	</span>
	<nav class="flex items-center gap-1">
		<Button class={navClass('/collections')} onclick={() => _goto('/collections')}>Collections</Button>
		<Button class={navClass('/logs')} onclick={() => _goto('/logs')}>Logs</Button>
		<Button class={navClass('/settings')} onclick={() => _goto('/settings')}>Settings</Button>
	</nav>
	<div class="ml-auto">
		<Button class="btn-ghost btn-sm text-primary-content" onclick={logout}>Logout</Button>
	</div>
</header>


