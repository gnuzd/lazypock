<script lang="ts">
	import { addableFieldTypes, fieldTypes, createField, type FieldDefinition } from '$lib/fieldTypes';
	import Dropdown from '$lib/components/Dropdown.svelte';
	import Button from '$lib/components/Button.svelte';

	let {
		fields = $bindable<FieldDefinition[]>([]),
	}: {
		fields: FieldDefinition[];
	} = $props();

	let open = $state(false);

	function addField(type: string) {
		const newField = createField(type, fields);
		const idx = fields.findLastIndex((f) => f.type !== 'autodate');
		if (type !== 'autodate' && idx >= 0) {
			fields.splice(idx + 1, 0, newField);
		} else {
			fields.push(newField);
		}
		open = false;
	}
</script>

<Dropdown bind:show={open} class="my-2">
	{#snippet trigger()}
		<Button class="w-full flex items-center justify-center gap-2 border-2 border-current bg-transparent text-base-content" onclick={() => open = !open}>
			<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>
			<span>New field</span>
		</Button>
	{/snippet}

	<div class="flex flex-row flex-wrap gap-0 p-1.5 min-w-[300px]">
		{#each addableFieldTypes as type (type)}
			<button
				type="button"
				class="w-1/4 p-2 my-0.5 cursor-pointer outline-none border-none bg-transparent text-base-content rounded-field flex items-center gap-2 text-sm text-left hover:bg-base-200 transition-[background] duration-(--animation-speed)"
				onmousedown={(e) => { e.preventDefault(); addField(type); }}
			>
				{#if type === 'text'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="4 7 4 4 20 4 20 7"/><line x1="9" y1="20" x2="15" y2="20"/><line x1="12" y1="4" x2="12" y2="20"/></svg>
				{:else if type === 'number'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="4" y1="9" x2="20" y2="9"/><line x1="4" y1="15" x2="20" y2="15"/><line x1="10" y1="3" x2="8" y2="21"/><line x1="16" y1="3" x2="14" y2="21"/></svg>
				{:else if type === 'bool'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="6" width="20" height="12" rx="6"/><circle cx="8" cy="12" r="2"/></svg>
				{:else if type === 'email'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z"/><polyline points="22,6 12,13 2,6"/></svg>
				{:else if type === 'url'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>
				{:else if type === 'date'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="18" rx="2" ry="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/></svg>
				{:else if type === 'select'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"/></svg>
				{:else if type === 'json'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14.5 3.5L17 6l-2.5 2.5"/><path d="M9.5 3.5L7 6l2.5 2.5"/><path d="M12 20l4-10"/><path d="M4 20h16"/></svg>
				{:else if type === 'file'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z"/><circle cx="12" cy="13" r="3"/></svg>
				{:else if type === 'relation'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/></svg>
				{:else if type === 'geoPoint'}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 10c0 7-9 13-9 13s-9-6-9-13a9 9 0 0 1 18 0z"/><circle cx="12" cy="10" r="3"/></svg>
				{:else}
					<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
				{/if}
				<span class="truncate">{fieldTypes[type]?.label || type}</span>
			</button>
		{/each}
	</div>
</Dropdown>
