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
		columns: {
			key: string;
			label: string;
			class?: string;
			render?: (row: Record<string, unknown>) => string;
		}[];
		rows: Record<string, unknown>[];
		onrowclick?: (row: Record<string, unknown>) => void;
		emptyLabel?: string;
		emptyActionLabel?: string;
		onemptyaction?: () => void;
	} = $props();
</script>

<div class="overflow-x-auto rounded-box border border-base-300 bg-base-100">
	<table class="w-full border-collapse text-sm">
		<thead>
			<tr class="bg-base-200 text-xs font-semibold tracking-wider text-base-content/60 uppercase">
				{#each columns as col (col.key)}
					<th
						class="border-b border-base-300 px-3.5 py-2.5 text-left whitespace-nowrap {col.class ??
							''}">{col.label}</th
					>
				{/each}
			</tr>
		</thead>
		<tbody>
			{#if rows.length === 0}
				<tr>
					<td colspan={columns.length} class="py-8 text-center opacity-50">
						{#if emptyLabel}
							<span class="mb-2 block text-sm">{emptyLabel}</span>
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
								class="max-w-60 truncate border-b border-base-200 px-3.5 py-2 {col.class ?? ''}"
								role={onrowclick ? 'button' : undefined}
								onclick={onrowclick ? () => onrowclick(row) : undefined}
							>
								{col.render ? col.render(row) : ((row[col.key] as string) ?? '—')}
							</td>
						{/each}
					</tr>
				{/each}
			{/if}
		</tbody>
	</table>
</div>
