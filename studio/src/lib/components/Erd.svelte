<script lang="ts">
	/**
	 * ERD ("Fields and relations") view — PocketBase's Collections overview
	 * tab. Renders each collection as a box (name + type + fields, relation
	 * fields highlighted) and draws SVG relation lines between boxes.
	 * Clicking a collection highlights its relations.
	 */
	let {
		collections = []
	}: {
		collections?: Record<string, unknown>[];
	} = $props();

	let containerEl = $state<HTMLDivElement | undefined>(undefined);
	let boxEls = $state<Map<string, HTMLElement>>(new Map());
	let size = $state({ w: 0, h: 0 });
	let active = $state<string | null>(null);

	$effect(() => {
		const el = containerEl;
		if (!el) return;
		const ro = new ResizeObserver(() => {
			const r = el.getBoundingClientRect();
			size = { w: r.width, h: r.height };
		});
		ro.observe(el);
		return () => ro.disconnect();
	});

	/** relation fields: [{ from: collection, to: collection, field: name, index }] */
	const relations = $derived.by(() => {
		const rels: { from: string; to: string; field: string; index: number }[] = [];
		for (const c of collections) {
			const fields = ((c.fields as Record<string, unknown>[]) ?? []).toSorted(
				(a, b) => ((a.sort_order as number) ?? 0) - ((b.sort_order as number) ?? 0)
			);
			fields.forEach((f, i) => {
				if (f.type === 'relation') {
					const target = ((f.options ?? {}) as Record<string, unknown>).collection as string;
					if (target && target !== c.name) {
						rels.push({ from: c.name as string, to: target, field: f.name as string, index: i });
					}
				}
			});
		}
		return rels;
	});

	const lines = $derived.by(() => {
		const containerRect = containerEl?.getBoundingClientRect();
		if (!containerRect || size.w === 0) return [];
		const out: {
			key: string;
			d: string;
			from: string;
			to: string;
			field: string;
			active: boolean;
			highlighted: boolean;
		}[] = [];
		for (const rel of relations) {
			const fromEl = boxEls.get(rel.from);
			const toEl = boxEls.get(rel.to);
			if (!fromEl || !toEl) continue;
			const fr = fromEl.getBoundingClientRect();
			const tr = toEl.getBoundingClientRect();
			const rowEl = fromEl.querySelector<HTMLElement>(
				`[data-field-row][data-field="${CSS.escape(rel.field)}"]`
			);
			const fromY = rowEl
				? rowEl.getBoundingClientRect().top - containerRect.top + rowEl.getBoundingClientRect().height / 2
				: fr.top - containerRect.top + 46 + rel.index * 26 + 13;
			const toY = tr.top - containerRect.top + tr.height / 2;
			const fromX = fr.right - containerRect.left;
			const toX = tr.left - containerRect.left;
			const dx = Math.max(40, (toX - fromX) / 2);
			const d = `M ${fromX} ${fromY} C ${fromX + dx} ${fromY}, ${toX - dx} ${toY}, ${toX} ${toY}`;
			out.push({
				key: `${rel.from}.${rel.field}->${rel.to}`,
				d,
				from: rel.from,
				to: rel.to,
				field: rel.field,
				active: active === rel.from || active === rel.to,
				highlighted: active === null || active === rel.from || active === rel.to
			});
		}
		return out;
	});

	const visibleCollections = $derived(collections);

	function typeLabel(type: string): string {
		if (type === 'auth') return 'auth';
		if (type === 'view') return 'view';
		return 'base';
	}

	function registerBoxAction(node: HTMLElement) {
		const name = node.dataset['boxName'] ?? '';
		if (name) boxEls.set(name, node);
		return {
			destroy() {
				if (name) boxEls.delete(name);
			}
		};
	}
</script>

<div class="relative h-full w-full overflow-auto" bind:this={containerEl}>
	<div class="inline-block p-6" style="min-width:{size.w}px">
		<div
			class="grid gap-6"
			style="grid-template-columns:repeat(auto-fill, minmax(230px, 1fr))"
		>
			{#each visibleCollections as c (c.id as string)}
				{@const name = c.name as string}
				{@const fields = ((c.fields as Record<string, unknown>[]) ?? []).toSorted(
					(a, b) => ((a.sort_order as number) ?? 0) - ((b.sort_order as number) ?? 0)
				)}
				<div
					class="erd-box cursor-pointer rounded-field border bg-base-200/70 transition-opacity"
					class:border-primary={active === name}
					class:opacity-40={active !== null && active !== name}
					role="button"
					tabindex="0"
					data-box-name={name}
					onclick={() => (active = active === name ? null : name)}
					onkeydown={(e) => {
						if (e.key === 'Enter') active = active === name ? null : name;
					}}
					use:registerBoxAction
				>
					<div
						class="flex items-center gap-2 border-b border-base-300 px-3 py-2 text-sm font-semibold"
					>
						{#if (c.type as string) === 'view'}
							<span class="text-info">◈</span>
						{:else if (c.type as string) === 'auth'}
							<span class="text-warning">👤</span>
						{:else}
							<span class="opacity-60">▤</span>
						{/if}
						<span class="truncate">{name}</span>
						<span class="ml-auto rounded bg-base-300 px-1.5 py-0.5 text-[10px] font-medium opacity-70"
							>{typeLabel(c.type as string)}</span
						>
					</div>
					<div class="py-1">
						{#each fields as f (f.id as string)}
							<div
								class="flex items-center gap-2 px-3 py-[3px] font-mono text-xs {f.type === 'relation'
									? 'bg-primary/10'
									: ''}"
								data-field-row
								data-field={f.name as string}
							>
								<span class="w-3 shrink-0 text-center opacity-50">
									{f.type === 'relation' ? '→' : ''}
								</span>
								<span class="truncate" class:font-medium={f.type === 'relation'}>
									{f.name as string}
								</span>
								<span class="ml-auto shrink-0 text-[10px] opacity-40">{f.type as string}</span>
							</div>
						{/each}
					</div>
				</div>
			{/each}
		</div>
	</div>

	{#if size.w > 0}
		<svg
			class="pointer-events-none absolute inset-0 h-full w-full"
			width={size.w}
			height={size.h}
		>
			{#each lines as line (line.key)}
				<path
					d={line.d}
					fill="none"
					stroke={line.active ? 'var(--color-primary)' : 'var(--color-base-content)'}
					stroke-opacity={line.highlighted ? (line.active ? 0.9 : 0.25) : 0.08}
					stroke-width={line.active ? 1.8 : 1.2}
				/>
			{/each}
		</svg>
	{/if}
</div>
