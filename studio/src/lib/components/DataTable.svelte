<script lang="ts">
	import Button from '$lib/components/Button.svelte';

let {
		columns,
		rows,
		onrowclick,
		emptyLabel = '',
		emptyActionLabel = '',
		onemptyaction
	}: {
		columns: { key: string; label: string; class?: string; render?: (row: Record<string, unknown>) => string }[];
		rows: Record<string, unknown>[];
		onrowclick?: (row: Record<string, unknown>) => void;
		emptyLabel?: string;
		emptyActionLabel?: string;
		onemptyaction?: () => void;
	} = $props();
</script>

<div class="overflow-x-auto border border-base-300 rounded-box bg-base-100">
	<table class="w-full border-collapse text-sm">
		<thead>
			<tr class="text-xs font-semibold uppercase tracking-wider text-base-content/60 bg-base-200">
				{#each columns as col (col.key)}
					<th class="px-3.5 py-2.5 text-left whitespace-nowrap border-b border-base-300 {col.class ?? ''}">{col.label}</th>
				{/each}
			</tr>
		</thead>
		<tbody>
			{#if rows.length === 0}
				<tr>
					<td colspan={columns.length} class="text-center py-8 opacity-50">
						{#if emptyLabel}
							<span class="block mb-2 text-sm">{emptyLabel}</span>
						{/if}
						{#if emptyActionLabel && onemptyaction}
							<Button class="btn-primary" onclick={onemptyaction}>{emptyActionLabel}</Button>
						{/if}
					</td>
				</tr>
			{:else}
				{#each rows as row (row.id)}
					<tr class="transition-[background] duration-(--animation-speed-fast) hover:bg-base-200">
						{#each columns as col (col.key)}
							<td
								class="px-3.5 py-2 border-b border-base-200 max-w-60 truncate {col.class ?? ''}"
								role={onrowclick ? 'button' : undefined}
								onclick={onrowclick ? () => onrowclick(row) : undefined}
							>
								{col.render ? col.render(row) : (row[col.key] as string ?? '—')}
							</td>
						{/each}
					</tr>
				{/each}
			{/if}
		</tbody>
	</table>
</div>
