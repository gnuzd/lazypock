<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import type { CollectionSchema } from 'lazypock-ts';
	import { Code2, FileText, Plus, RefreshCw, Search, Settings2 } from '@lucide/svelte';

	import Button from '#lib/components/ui/Button.svelte';
	import Input from '#lib/components/ui/Input.svelte';
	import { client } from '#lib/api/client.ts';
	import Badge from '#lib/components/ui/Badge.svelte';
	import { cn } from '#lib/utils/cn.ts';
	import { draw } from 'svelte/transition';
	import { drawer } from '#lib/utils/drawer.svelte.ts';
	import Rail from '../Rail.svelte';

	let collections = $state<CollectionSchema[]>([]);

	let selected = $derived(() => {
		const collectionName = page.url.searchParams.get('collection');
		return collections.find((item) => item.name === collectionName);
	});

	const fetchData = async () => {
		const res = await client.collections.getFullList();
		collections = res;
	};

	onMount(() => fetchData());
</script>

{#snippet searchIcon()}
	<Search size={16} class="text-base-content/50" />
{/snippet}

{#snippet header()}
	<div class="flex items-center justify-between border-b px-6 py-3 text-sm">
		<div class="flex items-center gap-2">
			<span class="font-medium text-muted">Collections</span>
			<span>/</span>
			<span class="font-medium">{selected()?.name}</span>
		</div>

		<div>
			<Button size="xs" buttonStyle="soft" variant="neutral" shape="square">
				<RefreshCw size={14} />
			</Button>
			<Button size="xs" variant="neutral" buttonStyle="soft">
				<Settings2 size={14} />
				Edit Schema
			</Button>

			<Button size="xs" variant="neutral" buttonStyle="soft">
				<Code2 size={14} />
				API Preview
			</Button>

			<Button size="xs" variant="neutral">
				<Plus size={14} />
				New Record
			</Button>
		</div>
	</div>
{/snippet}

<div class="flex flex-1 divide-x divide-base-300">
	<div class="flex w-[82vw] max-w-75 flex-col justify-between divide-y divide-base-300">
		<div class="w-full p-3">
			<Input size="sm" leadingIcon={searchIcon} />
		</div>

		<div class="flex flex-1 flex-col gap-1 overflow-auto p-2">
			{#each collections as item}
				<a
					href={`?collection=${item.name}`}
					class={cn(
						'flex cursor-pointer items-center justify-between rounded-field px-2.5 py-2 hover:bg-base-200/70',
						item.id === selected()?.id ? 'bg-base-200/70' : ''
					)}
				>
					<div class="flex items-center gap-2">
						<FileText size={14} />
						<span class="text-sm">{item.name}</span>
					</div>
					<Badge variant="default">{item.type}</Badge>
				</a>
			{/each}
		</div>
		<div class="p-3">
			<Button size="sm" variant="neutral" buttonStyle="outline" class="w-full">
				<Plus size={16} />
				New Collections
			</Button>
		</div>
	</div>

	<div class="flex-1">
		{@render header()}
	</div>
</div>
