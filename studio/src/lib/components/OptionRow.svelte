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

	let local = $state(value);

	// Notify parent when user edits
	$effect(() => {
		const v = local;
		if (v !== value) {
			onchange?.(v);
		}
	});
</script>

<div class="flex items-center gap-1">
	<div class="flex-1">
		<Input bind:value={local} placeholder="Option value" />
	</div>
	<button
		type="button"
		class="btn btn-ghost btn-sm btn-circle text-error/60 hover:text-error"
		onclick={onremove}
	>×</button>
</div>
