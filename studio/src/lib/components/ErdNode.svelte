<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import type { CollectionNode } from './erdTypes';

	let { data, selected }: NodeProps<CollectionNode> = $props();

	const fields = $derived(data.fields ?? []);

	// Measured handle positions: each relation field row gets a source handle
	// at the exact vertical center of its row (handles are absolutely
	// positioned against the node, so default 50% would land mid-node).
	let rowEls: (HTMLDivElement | undefined)[] = [];
	let handleTops = $state<Record<string, number>>({});

	$effect(() => {
		const tops: Record<string, number> = {};
		fields.forEach((f, i) => {
			if (f.type === 'relation') {
				const el = rowEls[i];
				if (el) tops[`field-${f.name}`] = el.offsetTop + el.offsetHeight / 2;
			}
		});
		handleTops = tops;
	});
</script>

<div
	class="min-w-[220px] overflow-hidden rounded-field border bg-base-200/90 text-sm shadow-sm transition-opacity"
	class:border-primary={selected}
	class:border-base-300={!selected}
>
	<div class="flex h-9 items-center gap-2 border-b border-base-300 px-3 font-semibold">
		<span class="opacity-60">
			{data.type === 'view' ? '◈' : data.type === 'auth' ? '👤' : '▤'}
		</span>
		<span class="truncate">{data.name}</span>
		<span class="ml-auto shrink-0 rounded bg-base-300 px-1.5 py-0.5 text-[10px] font-medium opacity-70"
			>{data.type}</span
		>
	</div>
	<div class="py-[2px]">
		{#each fields as f, i (f.id as string)}
			<div
				class="flex h-6 items-center gap-2 px-3 font-mono text-xs {f.type === 'relation'
					? 'bg-primary/10'
					: ''}"
				bind:this={rowEls[i]}
			>
				{#if f.type === 'relation'}
					<Handle
						type="source"
						position={Position.Right}
						id={`field-${f.name}`}
						style={`top: ${handleTops[`field-${f.name}`] ?? 0}px`}
						class="!h-2 !w-2 !border-0 !bg-primary"
					/>
				{/if}
				<span class="w-3 shrink-0 text-center opacity-50">{f.type === 'relation' ? '→' : ''}</span>
				<span class="truncate" class:font-medium={f.type === 'relation'}>{f.name as string}</span>
				<span class="ml-auto shrink-0 text-[10px] opacity-40">{f.type as string}</span>
			</div>
		{/each}
	</div>
	<Handle
		type="target"
		position={Position.Left}
		id="in"
		style="top: 18px"
		class="!h-2 !w-2 !border-0 !bg-base-content/60"
	/>
</div>
