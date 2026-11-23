<script lang="ts">
	import Box from '@lucide/svelte/icons/box';
	import Database from '@lucide/svelte/icons/database';
	import Activity from '@lucide/svelte/icons/activity';
	import Cog from '@lucide/svelte/icons/cog';
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

		<nav class="flex flex-col">
			{#each links as item}
				<Button shape="square" buttonStyle="ghost" variant="neutral">
					<item.icon size={20} />
				</Button>
			{/each}
		</nav>
	</div>
</aside>
