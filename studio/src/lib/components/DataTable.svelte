<script lang="ts">
	import type { Snippet } from 'svelte';
	import { fly } from 'svelte/transition';
	import Button from '$lib/components/Button.svelte';

	type Column = {
		key: string;
		label: string;
		class?: string;
		render?: (row: Record<string, unknown>) => string;
		/** Return thumbnail URLs for this cell; rendered as <img> rows (no {@html}). */
		thumbs?: (row: Record<string, unknown>) => string[];
	};

	let {
		columns,
		rows,
		onrowclick,
		emptyLabel = '',
		emptyActionLabel = '',
		onemptyaction,
		loading = false,
		zebra = false,
		selectable = false,
		fillHeight = false,
		selectedIds = $bindable([] as string[]),
		cell,
		selectionActions
	}: {
		columns: Column[];
		rows: Record<string, unknown>[];
		onrowclick?: (row: Record<string, unknown>) => void;
		emptyLabel?: string;
		emptyActionLabel?: string;
		onemptyaction?: () => void;
		loading?: boolean;
		zebra?: boolean;
		selectable?: boolean;
		/** Fill the parent's height (flex-1 container) with an internal scroll area. */
		fillHeight?: boolean;
		selectedIds?: string[];
		cell?: Snippet<[row: Record<string, unknown>, col: Column]>;
		/** Extra action buttons rendered inside the floating selection bar. */
		selectionActions?: Snippet;
	} = $props();

	const colspan = $derived(columns.length + (selectable ? 1 : 0));

	const allSelected = $derived(
		rows.length > 0 && rows.every((r) => selectedIds.includes(r.id as string))
	);

	function isSelected(row: Record<string, unknown>): boolean {
		return selectedIds.includes(row.id as string);
	}

	function toggleRow(row: Record<string, unknown>, checked: boolean) {
		const id = row.id as string;
		selectedIds = checked ? [...selectedIds, id] : selectedIds.filter((x) => x !== id);
	}

	function toggleAll(checked: boolean) {
		selectedIds = checked ? rows.map((r) => r.id as string) : [];
	}
</script>

<div
	class={fillHeight
		? 'flex h-full flex-col overflow-hidden rounded-box border border-base-300 bg-base-100'
		: 'overflow-auto rounded-box border border-base-300 bg-base-100'}
>
	<div class={fillHeight ? 'min-h-0 flex-1 overflow-auto' : ''}>
		<table class="w-full border-collapse text-sm">
			<thead>
				<tr class="bg-base-200 text-xs font-semibold tracking-wider text-base-content/60 uppercase">
					{#if selectable}
						<th
							class="sticky top-0 z-10 w-10 border-b border-base-300 bg-base-200 px-3.5 py-2.5"
						>
							<input
								type="checkbox"
								class="checkbox checkbox-sm"
								checked={allSelected}
								onchange={(e) => toggleAll(e.currentTarget.checked)}
								title="Select all rows"
							/>
						</th>
					{/if}
					{#each columns as col (col.key)}
						<th
							class="sticky top-0 z-10 border-b border-base-300 bg-base-200 px-3.5 py-2.5 text-left whitespace-nowrap {col.class ??
								''}">{col.label}</th
						>
					{/each}
				</tr>
			</thead>
			<tbody>
				{#if loading}
					<tr>
						<td {colspan} class="py-8 text-center opacity-50">Loading...</td>
					</tr>
				{:else if rows.length === 0}
					<tr>
						<td {colspan} class="py-8 text-center opacity-50">
							{#if emptyLabel}
								<span class="mb-2 block text-sm">{emptyLabel}</span>
							{/if}
							{#if emptyActionLabel && onemptyaction}
								<Button class="btn-primary" onclick={onemptyaction}>{emptyActionLabel}</Button>
							{/if}
						</td>
					</tr>
				{:else}
					{#each rows as row, i (row.id ?? i)}
						<tr
							class="transition-[background] duration-(--animation-speed-fast) hover:bg-base-200 {isSelected(
								row
							)
								? 'bg-base-200/50'
								: ''} {zebra && i % 2 === 1 ? 'bg-base-100/50' : ''}"
						>
							{#if selectable}
								<td class="w-10 border-b border-base-200 px-3.5 py-2">
									<input
										type="checkbox"
										class="checkbox checkbox-sm"
										checked={isSelected(row)}
										onchange={(e) => toggleRow(row, e.currentTarget.checked)}
										onclick={(e) => e.stopPropagation()}
									/>
								</td>
							{/if}
							{#each columns as col (col.key)}
								<td
									class="max-w-60 truncate border-b border-base-200 px-3.5 py-2 {col.class ?? ''}"
									role={onrowclick ? 'button' : undefined}
									onclick={onrowclick ? () => onrowclick(row) : undefined}
								>
									{#if col.thumbs}
										{@const thumbUrls = col.thumbs(row)}
										{#if thumbUrls.length}
											<div class="file-thumbs">
												{#each thumbUrls as url (url)}
													<img src={url} alt="" class="file-thumb" />
												{/each}
											</div>
										{:else}
											<span class="opacity-50">—</span>
										{/if}
									{:else if cell}
										{@render cell(row, col)}
									{:else}
										{col.render ? col.render(row) : ((row[col.key] as string) ?? '—')}
									{/if}
								</td>
							{/each}
						</tr>
					{/each}
				{/if}
			</tbody>
		</table>
	</div>
</div>

{#if selectable && selectedIds.length > 0}
	<div class="pointer-events-none fixed inset-x-0 bottom-4 z-50 flex justify-center px-4">
		<div
			transition:fly={{ y: 80, duration: 200 }}
			class="pointer-events-auto flex items-center gap-2 rounded-box border border-base-300 bg-base-100 px-4 py-2.5 shadow-lg"
		>
			<span class="text-sm font-medium">{selectedIds.length} selected</span>
			{#if selectionActions}
				<span class="mx-1 h-5 w-px bg-base-300"></span>
				{@render selectionActions()}
			{/if}
			<Button class="btn-ghost btn-sm" onclick={() => (selectedIds = [])}>Reset</Button>
		</div>
	</div>
{/if}

<style>
	.file-thumbs {
		display: flex;
		align-items: center;
		gap: 4px;
	}

	.file-thumb {
		width: 32px;
		height: 32px;
		object-fit: cover;
		border-radius: 6px;
		background: var(--color-base-200, #f0f0f0);
	}
</style>
