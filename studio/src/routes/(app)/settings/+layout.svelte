<script lang="ts">
	import { base } from '$app/paths';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import { RefreshCw } from '@lucide/svelte';
	import {
		Archive,
		Cog,
		Database,
		Download,
		HardDrive,
		KeyRound,
		Mail,
		Upload
	} from '@lucide/svelte';

	let { children } = $props();

	interface NavItem {
		id: string;
		label: string;
		icon: typeof Cog;
		href: string;
	}

	const systemItems: NavItem[] = [
		{ id: 'application', label: 'Application', icon: Cog, href: '/settings/application' },
		{ id: 'mail', label: 'Mail', icon: Mail, href: '/settings/mail' },
		{ id: 'files', label: 'Files Storage', icon: HardDrive, href: '/settings/files' },
		{ id: 'api-keys', label: 'API Keys', icon: KeyRound, href: '/settings/api-keys' },
		{ id: 'backups', label: 'Backups', icon: Archive, href: '/settings/backups' },
		{ id: 'cron', label: 'Cron', icon: RefreshCw, href: '/settings/cron' }
	];

	const syncItems: NavItem[] = [
		{ id: 'export', label: 'Export Collections', icon: Upload, href: '/settings/export' },
		{ id: 'import', label: 'Import Collections', icon: Download, href: '/settings/import' }
	];

	const debugItems: NavItem[] = [
		{ id: 'sql', label: 'SQL Console', icon: Database, href: '/settings/sql' }
	];

	const groups = [
		{ title: 'System', items: systemItems },
		{ title: 'Sync', items: syncItems },
		{ title: 'Debug', items: debugItems }
	];

	function isActive(href: string): boolean {
		return page.url.pathname.endsWith(href);
	}

	function _goto(href: string) {
		// eslint-disable-next-line svelte/no-navigation-without-resolve
		goto(base + href);
	}
</script>

{#snippet bodyContent()}
	{#each groups as group (group.title)}
		<div
			class="mt-2 mb-1 px-3.5 pt-2 text-[10px] font-semibold tracking-wider text-base-content/40 uppercase first:mt-0"
		>
			{group.title}
		</div>
		{#each group.items as s (s.id)}
			<button
				class="mx-1.5 flex w-[calc(100%-12px)] cursor-pointer items-center gap-2 rounded-field border-none px-3 py-1.5 text-left text-sm text-base-content transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
				class:bg-base-200={isActive(s.href)}
				class:font-medium={isActive(s.href)}
				role="button"
				tabindex="0"
				onclick={() => _goto(s.href)}
				onkeydown={(e) => {
					if (e.key === 'Enter') _goto(s.href);
				}}
			>
				<s.icon class="h-4 w-4 shrink-0 opacity-60" />
				<span class="flex-1 truncate">{s.label}</span>
			</button>
		{/each}
	{/each}
{/snippet}

<!-- Layout: sidebar left, main content right -->
<div class="flex flex-1 overflow-hidden">
	<Sidebar body={bodyContent} />
	<main class="flex-1 overflow-auto p-6">
		<div
			class="mx-auto"
			style="max-width: {page.url.pathname.endsWith('/export') ||
			page.url.pathname.endsWith('/import')
				? '1200px'
				: page.url.pathname.endsWith('/sql')
					? '100%'
					: '870px'}"
		>
			{@render children()}
		</div>
	</main>
</div>
