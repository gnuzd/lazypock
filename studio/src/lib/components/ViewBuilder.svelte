<script lang="ts">
	import { client } from '$lib/client';
	import Button from '$lib/components/Button.svelte';
	import Erd from '$lib/components/Erd.svelte';
	import Modal from '$lib/components/Modal.svelte';
	import Select from '$lib/components/Select.svelte';
	import type { ViewBuilderField, ViewBuilderSpec } from '$lib/viewBuilder';

	/**
	 * No-code view builder. Lets the user pick a source collection, tick the
	 * columns they want (including fields reached through relations) and see
	 * the generated SQL + a live sample before saving.
	 *
	 * `spec` is the only state: every change reassigns it, and a debounced
	 * effect previews it against the backend generator.
	 */
	interface FieldDef {
		name: string;
		type: string;
		hidden?: boolean;
		sort_order?: number;
		options?: Record<string, unknown>;
	}

	let {
		spec = $bindable(),
		collections = [],
		excludeName = ''
	}: {
		spec: ViewBuilderSpec;
		collections?: Record<string, unknown>[];
		/** Collection to hide from the source list (the one being edited). */
		excludeName?: string;
	} = $props();

	let previewState = $state<'idle' | 'testing' | 'ok' | 'error'>('idle');
	let previewQuery = $state('');
	let previewFields = $state<{ name: string; type: string }[]>([]);
	let previewSample = $state<Record<string, unknown>[]>([]);
	let previewError = $state('');
	let previewTimer: ReturnType<typeof setTimeout> | undefined;
	let showErd = $state(false);
	let sourceValue = $state(spec.source ?? '');

	// Keep the Select in sync when the spec is loaded (or changed by the ERD).
	$effect(() => {
		if (spec.source !== sourceValue) sourceValue = spec.source ?? '';
	});

	const selectableCollections = $derived(collections.filter((c) => c.name !== excludeName));

	const sourceOptions = $derived(
		selectableCollections.map((c) => ({ value: c.name as string, label: c.name as string }))
	);

	const sourceColl = $derived(collections.find((c) => c.name === spec.source) ?? null);

	const sourceFields = $derived.by((): FieldDef[] => {
		const raw = (sourceColl?.fields as FieldDef[]) ?? [];
		return raw
			.filter((f) => f.name !== 'id' && f.type !== 'password' && !f.hidden)
			.sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
	});

	const relationFields = $derived(sourceFields.filter((f) => f.type === 'relation'));

	function fields(): ViewBuilderField[] {
		return spec.fields ?? [];
	}

	function relations() {
		return spec.relations ?? [];
	}

	function relationTarget(field: FieldDef): string {
		return String(field.options?.collection ?? '');
	}

	function targetFields(field: FieldDef): FieldDef[] {
		const target = collections.find((c) => c.name === relationTarget(field));
		const raw = (target?.fields as FieldDef[]) ?? [];
		return raw
			.filter((f) => f.name !== 'id' && f.type !== 'password' && !f.hidden)
			.sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
	}

	function applySource(name: string) {
		if (!name || name === spec.source) return;
		sourceValue = name;
		spec = { source: name, relations: [], fields: [] };
	}

	function isBaseSelected(name: string): boolean {
		return fields().some((f) => f.source === spec.source && f.name === name);
	}

	function toggleBase(name: string) {
		if (isBaseSelected(name)) {
			spec = {
				...spec,
				fields: fields().filter((f) => !(f.source === spec.source && f.name === name))
			};
		} else {
			spec = { ...spec, fields: [...fields(), { source: spec.source, name }] };
		}
	}

	function relationFor(field: string) {
		return relations().find((r) => r.field === field);
	}

	function nextAlias(): string {
		const used = new Set(relations().map((r) => r.alias));
		let i = 1;
		while (used.has(`t${i}`)) i += 1;
		return `t${i}`;
	}

	function isRelatedSelected(relationField: string, name: string): boolean {
		const rel = relationFor(relationField);
		return !!rel && fields().some((f) => f.source === rel.alias && f.name === name);
	}

	function toggleRelated(relationField: string, name: string) {
		const rel = relationFor(relationField);

		if (rel && isRelatedSelected(relationField, name)) {
			const nextFields = fields().filter((f) => !(f.source === rel.alias && f.name === name));
			const stillUsed = nextFields.some((f) => f.source === rel.alias);
			const nextRelations = stillUsed
				? relations()
				: relations().filter((r) => r.alias !== rel.alias);
			spec = { ...spec, fields: nextFields, relations: nextRelations };
		} else {
			const alias = rel?.alias ?? nextAlias();
			const nextRelations = rel ? relations() : [...relations(), { alias, field: relationField }];
			spec = {
				...spec,
				relations: nextRelations,
				fields: [...fields(), { source: alias, name }]
			};
		}
	}

	function indexOfField(source: string, name: string): number {
		return fields().findIndex((f) => f.source === source && f.name === name);
	}

	function setAlias(index: number, value: string) {
		const next = [...fields()];
		if (!next[index]) return;
		if (value.trim()) {
			next[index] = { ...next[index], as: value };
		} else {
			const { as: _as, ...rest } = next[index];
			void _as;
			next[index] = rest;
		}
		spec = { ...spec, fields: next };
	}

	function cell(value: unknown): string {
		if (value === null || value === undefined) return '';
		return typeof value === 'object' ? JSON.stringify(value) : String(value);
	}

	// Debounced live preview against the backend generator.
	$effect(() => {
		const current = {
			source: spec.source,
			relations: spec.relations ?? [],
			fields: spec.fields ?? [],
			sort: spec.sort,
			limit: spec.limit
		};
		clearTimeout(previewTimer);

		if (!current.source) {
			previewState = 'idle';
			previewQuery = '';
			previewFields = [];
			previewSample = [];
			previewError = '';
			return;
		}

		previewState = 'testing';
		previewTimer = setTimeout(async () => {
			try {
				const res = (await client.http.post('/collections/meta/preview-view-builder', {
					viewBuilder: current
				})) as {
					query: string;
					fields: { name: string; type: string }[];
					sample: Record<string, unknown>[];
				};
				previewQuery = res.query ?? '';
				previewFields = res.fields ?? [];
				previewSample = res.sample ?? [];
				previewError = '';
				previewState = 'ok';
			} catch (e) {
				previewError =
					((e as { message?: string })?.message ?? '').replace(
						/^Invalid view builder spec\. Raw error:\s*/,
						''
					) || 'Invalid view definition';
				previewQuery = '';
				previewFields = [];
				previewSample = [];
				previewState = 'error';
			}
		}, 300);
	});
</script>

<div class="flex flex-col gap-3">
	<div class="flex flex-col gap-1">
		<span class="text-sm font-medium">Source collection</span>
		<div class="flex items-center gap-2">
			<Select
				options={sourceOptions}
				bind:value={sourceValue}
				onchange={(v) => applySource(String(v ?? ''))}
				placeholder="Pick a collection…"
				class="flex-1"
			/>
			<Button class="btn-outline btn-sm shrink-0" onclick={() => (showErd = true)}>
				Pick from diagram
			</Button>
		</div>
	</div>

	{#if !spec.source}
		<p class="rounded-field border border-base-300 bg-base-200/40 p-3 text-xs text-base-content/60">
			Pick a source collection to start. The view's <code>id</code> column is taken from the source record
			automatically, and relation fields can pull in columns from their target collection.
		</p>
	{:else}
		<div class="flex flex-col gap-1">
			<p class="text-sm font-medium">Fields</p>
			<div class="overflow-hidden rounded-field border border-base-300">
				<div class="flex items-center gap-2 border-b border-base-300 bg-base-200/40 px-3 py-2">
					<input type="checkbox" class="checkbox checkbox-sm" checked disabled />
					<span class="font-mono text-sm">id</span>
					<span class="ml-auto text-xs text-base-content/50">always included</span>
				</div>
				{#each sourceFields as field (field.name)}
					{@const index = indexOfField(spec.source, field.name)}
					<div class="flex items-center gap-2 border-b border-base-200 px-3 py-1.5 last:border-b-0">
						<input
							type="checkbox"
							class="checkbox checkbox-sm"
							checked={index >= 0}
							onchange={() => toggleBase(field.name)}
						/>
						<span class="font-mono text-sm">{field.name}</span>
						<span class="text-xs text-base-content/40">{field.type}</span>
						{#if index >= 0}
							<input
								class="input input-xs ml-auto w-44 font-mono"
								placeholder={field.name}
								value={spec.fields[index]?.as ?? ''}
								oninput={(e) => setAlias(index, e.currentTarget.value)}
							/>
						{/if}
					</div>
				{/each}
			</div>
		</div>

		{#if relationFields.length > 0}
			<div class="flex flex-col gap-1">
				<p class="text-sm font-medium">
					Related fields
					<span class="font-normal text-base-content/50">(joined collections)</span>
				</p>
				{#each relationFields as rel (rel.name)}
					{@const target = relationTarget(rel)}
					<details class="rounded-field border border-base-300">
						<summary class="cursor-pointer px-3 py-2 text-sm">
							{rel.name} → <span class="font-mono text-xs">{target || 'unknown'}</span>
						</summary>
						<div class="border-t border-base-300 px-3 py-2">
							{#each targetFields(rel) as targetField (targetField.name)}
								{@const alias = relationFor(rel.name)?.alias ?? ''}
								{@const index = alias ? indexOfField(alias, targetField.name) : -1}
								<div class="flex items-center gap-2 py-1">
									<input
										type="checkbox"
										class="checkbox checkbox-sm"
										checked={index >= 0}
										disabled={!target}
										onchange={() => toggleRelated(rel.name, targetField.name)}
									/>
									<span class="font-mono text-sm">{targetField.name}</span>
									<span class="text-xs text-base-content/40">{targetField.type}</span>
									{#if index >= 0}
										<input
											class="input input-xs ml-auto w-44 font-mono"
											placeholder={`${rel.name}_${targetField.name}`}
											value={spec.fields[index]?.as ?? ''}
											oninput={(e) => setAlias(index, e.currentTarget.value)}
										/>
									{/if}
								</div>
							{/each}
						</div>
					</details>
				{/each}
			</div>
		{/if}

		<div class="grid grid-cols-2 gap-3">
			<label class="flex flex-col gap-1 text-sm">
				<span class="font-medium">Sort</span>
				<input
					class="input input-sm font-mono"
					placeholder="-created_at, title"
					value={spec.sort ?? ''}
					oninput={(e) => (spec = { ...spec, sort: e.currentTarget.value })}
				/>
			</label>
			<label class="flex flex-col gap-1 text-sm">
				<span class="font-medium">Limit</span>
				<input
					class="input input-sm font-mono"
					type="number"
					min="1"
					placeholder="No limit"
					value={spec.limit ?? ''}
					oninput={(e) => {
						const raw = e.currentTarget.value;
						const parsed = Number(raw);
						spec = {
							...spec,
							limit: raw === '' || !Number.isFinite(parsed) ? undefined : parsed
						};
					}}
				/>
			</label>
		</div>

		<div class="flex flex-col gap-2 rounded-field border border-base-300 bg-base-200/30 p-3">
			<div class="flex items-center gap-2">
				<span class="text-sm font-medium">Preview</span>
				{#if previewState === 'testing'}
					<span class="text-xs text-base-content/50">Testing…</span>
				{:else if previewState === 'ok'}
					<span class="text-xs text-success">✓ Valid</span>
				{:else if previewState === 'error'}
					<span class="text-xs text-error">✗ Invalid</span>
				{/if}
			</div>

			{#if previewState === 'error'}
				<pre
					class="overflow-auto rounded-field border border-error/30 bg-error/10 p-2 font-mono text-xs whitespace-pre-wrap text-error">{previewError}</pre>
			{/if}

			{#if previewState === 'ok'}
				<div class="flex flex-wrap gap-1">
					{#each previewFields as f (f.name)}
						<span class="rounded bg-base-300 px-1.5 py-0.5 font-mono text-xs">{f.name}</span>
						<span class="mr-1 text-xs text-base-content/50">{f.type}</span>
					{/each}
				</div>

				{#if previewSample.length > 0}
					<div class="max-h-56 overflow-auto rounded-field border border-base-300">
						<table class="w-full border-collapse text-xs">
							<thead>
								<tr class="bg-base-200 text-left">
									{#each previewFields as f (f.name)}
										<th
											class="border-b border-base-300 px-2 py-1 font-mono font-normal whitespace-nowrap"
											>{f.name}</th
										>
									{/each}
								</tr>
							</thead>
							<tbody>
								{#each previewSample.slice(0, 5) as row, rowIndex (rowIndex)}
									<tr class="border-b border-base-200 last:border-b-0">
										{#each previewFields as f (f.name)}
											<td class="max-w-48 truncate px-2 py-1 font-mono">{cell(row[f.name])}</td>
										{/each}
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{:else}
					<p class="text-xs text-base-content/50">
						No records yet — the columns above are what the view will expose.
					</p>
				{/if}

				<details>
					<summary class="cursor-pointer text-xs text-base-content/60">Show generated SQL</summary>
					<pre
						class="mt-1 overflow-auto rounded-field border border-base-300 bg-base-100 p-2 font-mono text-xs"><code
							>{previewQuery}</code
						></pre>
				</details>
			{/if}
		</div>
	{/if}
</div>

<Modal bind:show={showErd} title="Pick a source collection">
	<p class="mb-2 text-xs text-base-content/60">
		Click a collection in the diagram to use it as the view source.
	</p>
	<div class="relative h-[55vh] w-full">
		<Erd
			collections={selectableCollections}
			onselect={(name) => {
				if (name) {
					applySource(name);
					showErd = false;
				}
			}}
		/>
	</div>
</Modal>
