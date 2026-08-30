<script lang="ts">
	import { tick } from 'svelte';
	import {
		SvelteFlow,
		Controls,
		Background,
		useSvelteFlow,
		useStore,
		type Edge
	} from '@xyflow/svelte';
	import ErdNode from './ErdNode.svelte';
	import type { CollectionNode } from './erdTypes';

	/**
	 * The actual Svelte Flow instance. Runs inside the SvelteFlowProvider
	 * (see Erd.svelte) so `useStore` can drive the flow's store — nodes and
	 * edges are synced through store assignments (no `bind:`, which would loop
	 * with the reactive graph build). Selection highlights connected edges.
	 */
	let {
		collections = []
	}: {
		collections?: Record<string, unknown>[];
	} = $props();

	const { fitView } = useSvelteFlow();
	// Re-read the store reactively — SvelteFlow replaces the provider's
	// initial store with its own once it mounts.
	const store = $derived(useStore());

	const nodeTypes = { collection: ErdNode };

	const COLS = 3;
	const NODE_W = 240;
	const GAP_X = 90;
	const GAP_Y = 60;

	const BASE_EDGE_STYLE =
		'stroke: var(--color-base-content); stroke-opacity: 0.45; stroke-width: 1.5';
	const DIM_EDGE_STYLE =
		'stroke: var(--color-base-content); stroke-opacity: 0.1; stroke-width: 1.5';
	const BRIGHT_EDGE_STYLE =
		'stroke: var(--color-primary); stroke-opacity: 1; stroke-width: 2';

	let selectedIds = new Set<string>();
	let hasSelection = $state(false);
	let flowReady = $state(false);

	function sortFields(fields: Record<string, unknown>[]): Record<string, unknown>[] {
		return [...(fields ?? [])].sort(
			(a, b) => ((a.sort_order as number) ?? 0) - ((b.sort_order as number) ?? 0)
		);
	}

	const nodes = $derived.by((): CollectionNode[] => {
		const nodeDefs: CollectionNode[] = [];
		let x = 0;
		let y = 0;
		let col = 0;
		let rowMaxH = 0;

		for (const c of collections) {
			const fields = sortFields((c.fields as Record<string, unknown>[]) ?? []);
			const nodeH = 38 + Math.max(1, fields.length) * 24 + 6;

			if (col > 0 && col % COLS === 0) {
				x = 0;
				y += rowMaxH + GAP_Y;
				rowMaxH = 0;
			}

			nodeDefs.push({
				id: c.name as string,
				type: 'collection',
				position: { x, y },
				data: {
					name: c.name as string,
					type: (c.type as string) ?? 'base',
					fields
				}
			});

			x += NODE_W + GAP_X;
			col += 1;
			rowMaxH = Math.max(rowMaxH, nodeH);
		}

		return nodeDefs;
	});

	const edges = $derived.by((): Edge[] => {
		const names = new Set(collections.map((c) => c.name as string));
		const edgeDefs: Edge[] = [];

		for (const c of collections) {
			const fields = sortFields((c.fields as Record<string, unknown>[]) ?? []);
			fields.forEach((f) => {
				if (f.type !== 'relation') return;
				const target = ((f.options ?? {}) as Record<string, unknown>).collection as string;
				if (!target || target === c.name || !names.has(target)) return;
				const connected = selectedIds.has(c.name as string) || selectedIds.has(target);

				edgeDefs.push({
					id: `${c.name}.${f.name}->${target}`,
					source: c.name as string,
					target,
					sourceHandle: `field-${f.name}`,
					targetHandle: 'in',
					label: f.name as string,
					labelStyle:
						'fill: var(--color-base-content); font-size: 10px; font-family: monospace',
					style: !hasSelection
						? BASE_EDGE_STYLE
						: connected
							? BRIGHT_EDGE_STYLE
							: DIM_EDGE_STYLE
				});
			});
		}

		return edgeDefs;
	});

	// Called by SvelteFlow once it has mounted and replaced the provider store.
	function onInit() {
		flowReady = true;
	}

	// Sync nodes + re-fit whenever the collections change (nodes derived only
	// re-computes on collections changes, so this doesn't fight the selection).
	$effect(() => {
		const s = store;
		const n = nodes;
		void n;
		if (!flowReady) return;
		s.nodes = nodes;
		queueMicrotask(async () => {
			await tick();
			await fitView({ padding: 0.15 });
		});
	});

	// Sync edges whenever they change (data or selection highlight).
	$effect(() => {
		const s = store;
		const e = edges;
		void e;
		if (!flowReady) return;
		s.edges = edges;
	});

	function onSelectionChange(sel: { nodes: { id: string }[] }) {
		selectedIds = new Set((sel.nodes ?? []).map((n) => n.id));
		hasSelection = selectedIds.size > 0;
	}
</script>

<SvelteFlow
	{nodeTypes}
	fitView={false}
	minZoom={0.15}
	maxZoom={2.5}
	nodesConnectable={false}
	nodesDraggable
	zoomOnScroll
	zoomOnDoubleClick
	panOnDrag
	elementsSelectable
	class="h-full w-full bg-base-100 {hasSelection ? 'has-selection' : ''}"
	oninit={onInit}
	onselectionchange={onSelectionChange}
>
	<Background gap={26} />
	<Controls position="bottom-left" />
</SvelteFlow>

<style>
	/* Dim non-selected nodes when something is selected (PB-style focus). */
	:global(.has-selection .svelte-flow__node:not(.selected)) {
		opacity: 0.35;
		transition: opacity 120ms ease;
	}
</style>
