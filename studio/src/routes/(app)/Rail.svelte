<script lang="ts">
	import { page } from '$app/state';
	import { Box, Database, Activity, Cog } from '@lucide/svelte';

	import Button from '#lib/components/ui/Button.svelte';

	const links = [
		{ icon: Database, path: '/collections' },
		{ icon: Activity, path: '/logs' },
		{ icon: Cog, path: '/settings' }
	];
</script>

<aside class="hidden w-14 border-r py-3 md:block">
	<div class="flex flex-col items-center gap-4">
		<Button shape="square" variant="neutral">
			<Box size={20} />
		</Button>

		<div class="h-px w-6 bg-base-300"></div>

		<nav class="flex flex-col gap-1">
			{#each links as item}
				<Button
					shape="square"
					buttonStyle={page.url.pathname === item.path ? 'solid' : 'ghost'}
					variant="neutral"
					href={item.path}
				>
					<item.icon size={20} />
				</Button>
			{/each}
		</nav>
	</div>
</aside>
