<script lang="ts">
	import Dropdown from '$lib/components/Dropdown.svelte';

	type Option = { value: unknown; label: string };

	let {
		options,
		value = $bindable(),
		placeholder = 'Select…',
		size = 'md' as 'sm' | 'md',
		variant = 'bordered' as 'bordered' | 'plain',
		class: className = '',
		triggerClass = '',
		menuClass = '',
		disabled = false,
		onchange
	}: {
		options: Option[];
		value?: unknown;
		placeholder?: string;
		size?: 'sm' | 'md';
		/** 'bordered' shows a bordered trigger; 'plain' is transparent (for use inside a .field wrapper). */
		variant?: 'bordered' | 'plain';
		class?: string;
		triggerClass?: string;
		menuClass?: string;
		disabled?: boolean;
		onchange?: (value: unknown) => void;
	} = $props();

	let open = $state(false);

	const selected = $derived(options.find((o) => o.value === value));

	const triggerBase = $derived(
		variant === 'plain'
			? 'flex w-full cursor-pointer items-center gap-2 rounded-field px-3 text-left text-sm text-base-content disabled:opacity-50'
			: 'flex cursor-pointer items-center gap-2 rounded-field border-2 border-base-300 bg-base-100 px-3 text-left text-base-content transition-colors hover:border-primary disabled:opacity-50 ' +
					(size === 'sm'
						? 'h-7 min-w-[70px] text-xs'
						: 'h-[38px] min-w-[110px] text-sm')
	);

	function choose(option: Option) {
		if (option.value === value) {
			open = false;
			return;
		}
		value = option.value;
		onchange?.(option.value);
		open = false;
	}
</script>

<Dropdown bind:show={open} class={className}>
	{#snippet trigger()}
		<button
			type="button"
			class={triggerBase + ' ' + triggerClass}
			disabled={disabled}
			aria-haspopup="listbox"
		>
			<span class="min-w-0 flex-1 truncate">
				{#if selected}
					{selected.label}
				{:else}
					<span class="opacity-50">{placeholder}</span>
				{/if}
			</span>
			<svg
				width="14"
				height="14"
				viewBox="0 0 24 24"
				fill="none"
				stroke="currentColor"
				stroke-width="2"
				class="shrink-0"><polyline points="6 9 12 15 18 9" /></svg
			>
		</button>
	{/snippet}

	<div class="min-w-[160px] py-1 {menuClass}">
		{#each options as option (option.value)}
			<button
				type="button"
				class="flex w-full cursor-pointer items-center gap-2 rounded-field border-none bg-transparent px-3 py-2 text-left text-sm text-base-content transition-colors hover:bg-base-200"
				class:font-medium={option.value === value}
				onclick={() => choose(option)}
			>
				<span class="flex-1 truncate">{option.label}</span>
				{#if option.value === value}
					<svg
						width="14"
						height="14"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2.5"
						class="shrink-0 text-primary"><polyline points="20 6 9 17 4 12" /></svg
					>
				{/if}
			</button>
		{/each}
	</div>
</Dropdown>
