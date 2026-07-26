<script lang="ts">
	import Input from './Input.svelte';

	let {
		value = '',
		onchange,
		onremove
	}: {
		value?: string;
		onchange?: (v: string) => void;
		onremove?: () => void;
	} = $props();

	// Need writable local state because #each variables are read-only,
	// so we can't bind:value directly to <Input>
	// eslint-disable-next-line svelte/prefer-writable-derived
	let local = $state(value);

	// Sync parent value changes (e.g. reorder) into local
	$effect(() => {
		local = value;
	});

	// Push local edits to parent — skip when value matches (avoids mount loop)
	$effect(() => {
		if (local !== value) {
			onchange?.(local);
		}
	});
</script>

<div class="flex items-center gap-1">
	<div class="flex-1">
		<Input bind:value={local} placeholder="Option value" />
	</div>
	<button
		type="button"
		class="btn btn-ghost aspect-square px-0 text-error/60 hover:text-error"
		onclick={onremove}>×</button
	>
</div>
