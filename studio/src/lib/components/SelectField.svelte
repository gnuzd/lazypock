<script lang="ts">
	type Choice = string | { label: string; value: string };

	interface Props {
		choices: Choice[];
		maxSelect: number;
		value: unknown;
		disabled?: boolean;
		onOpen?: () => void;
	}

	let {
		choices = [],
		maxSelect = 1,
		value = $bindable(),
		disabled = false,
		onOpen
	}: Props = $props();

	const isMulti = $derived(maxSelect > 1);
	const selected = $derived(isMulti ? (value as string[]) || [] : [(value as string) || '']);
	const filtered = $derived(
		choices.filter((c) => {
			if (typeof c === 'string') {
				return !search || c.toLowerCase().includes(search.toLowerCase());
			}

			return !search || c.label.toLowerCase().includes(search.toLowerCase());
		})
	);

	let search = $state('');
	let open = $state(false);
	let wrapper: HTMLDivElement | undefined = $state();

	function toggle() {
		if (disabled) return;
		open = !open;
		search = '';
		if (open) {
			onOpen?.();
		}
	}

	function selectOption(choice: Choice) {
		const choiceValue = getValue(choice);

		if (isMulti) {
			const cur = (value as string[]) || [];
			if (cur.includes(choiceValue)) {
				const next = cur.filter((s) => s !== choiceValue);
				value = next.length > 0 ? next : null;
			} else {
				value = [...cur, choiceValue];
			}
		} else {
			value = choiceValue;
			open = false;
		}
	}

	function removeChip(choiceValue: string) {
		const cur = (value as string[]) || [];
		const next = cur.filter((s) => s !== choiceValue);
		value = next.length > 0 ? next : null;
	}

	function getValue(choice: Choice): string {
		return typeof choice === 'string' ? choice : choice.value;
	}

	function getLabel(choice: Choice): string {
		return typeof choice === 'string' ? choice : choice.label;
	}

	$effect(() => {
		if (open) {
			function handle(e: MouseEvent) {
				if (wrapper && !wrapper.contains(e.target as Node)) {
					open = false;
					search = '';
				}
			}
			document.addEventListener('mousedown', handle);
			return () => document.removeEventListener('mousedown', handle);
		}
	});
</script>

<div bind:this={wrapper} class="select-wrapper">
	<div
		class="select-display"
		onclick={toggle}
		role="button"
		tabindex="0"
		onkeydown={(e) => {
			if (e.key === 'Enter') toggle();
		}}
	>
		{#if isMulti}
			{#if selected.length > 0}
				<div class="selected-chips">
					{#each selected as s (s)}
						{@const label = getLabel(filtered.find((c) => getValue(c) === s) ?? s)}
						<span class="selected-chip">
							{label}
							<button type="button" class="chip-remove" onclick={() => removeChip(s)}>×</button>
						</span>
					{/each}
				</div>
			{:else}
				<span class="select-placeholder">Select options...</span>
			{/if}
		{:else}
			<span class:select-placeholder={!selected[0]}>{selected[0] || 'Select...'}</span>
		{/if}
		<svg
			width="14"
			height="14"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			stroke-width="2"
			class="select-chevron"
			class:open><polyline points="6 9 12 15 18 9" /></svg
		>
	</div>
	{#if open}
		<div class="select-dropdown">
			<input type="text" class="select-search" bind:value={search} placeholder="Search..." />
			<div class="select-options">
				{#each filtered as choice (typeof choice === 'string' ? choice : choice.value)}
					{@const choiceValue = typeof choice === 'string' ? choice : choice.value}
					{@const choiceLabel = typeof choice === 'string' ? choice : choice.label}
					<!-- svelte-ignore a11y_click_events_have_key_events -->
					<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
					<label
						class="select-option"
						class:selected={selected.includes(choiceValue)}
						onclick={() => selectOption(choiceValue)}
					>
						{#if isMulti}
							<input
								type="checkbox"
								checked={selected.includes(choiceValue)}
								onchange={() => selectOption(choiceValue)}
							/>
						{/if}
						<span>{choiceLabel}</span>
					</label>
				{/each}
				{#if filtered.length === 0}
					<span class="select-no-results">No matching options</span>
				{/if}
			</div>
		</div>
	{/if}
</div>

<style>
	.select-wrapper {
		position: relative;
	}
	.select-display {
		display: flex;
		align-items: center;
		gap: 4px;
		padding: 8px 32px 8px 12px;
		cursor: pointer;
		min-height: 38px;
		position: relative;
	}
	.select-display:hover {
		background: color-mix(in oklab, var(--color-base-content) 4%, transparent);
	}
	.select-placeholder {
		opacity: 0.45;
		font-size: 0.9375rem;
	}
	.select-chevron {
		position: absolute;
		right: 10px;
		top: 50%;
		transform: translateY(-50%);
		opacity: 0.4;
		transition: transform 0.15s;
	}
	.select-chevron.open {
		transform: translateY(-50%) rotate(180deg);
	}
	.selected-chips {
		display: flex;
		flex-wrap: wrap;
		gap: 3px;
		flex: 1;
	}
	.selected-chip {
		display: inline-flex;
		align-items: center;
		gap: 2px;
		padding: 1px 4px 1px 8px;
		font-size: 0.8125rem;
		border-radius: 999px;
		background: color-mix(in oklab, var(--color-primary) 20%, var(--color-base-100));
		color: var(--color-primary);
		border: 1px solid color-mix(in oklab, var(--color-primary) 30%, transparent);
	}
	.chip-remove {
		background: none;
		border: none;
		cursor: pointer;
		padding: 0 2px;
		font-size: 1rem;
		line-height: 1;
		color: inherit;
		opacity: 0.6;
		border-radius: 999px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
	}
	.chip-remove:hover {
		opacity: 1;
	}
	.select-dropdown {
		position: absolute;
		z-index: 50;
		left: 0;
		right: 0;
		top: 100%;
		margin-top: 2px;
		background: var(--color-base-100);
		border: 2px solid var(--color-primary);
		border-radius: var(--radius-field);
		box-shadow: 0 4px 16px rgba(0, 0, 0, 0.15);
		overflow: hidden;
	}
	.select-search {
		width: 100%;
		padding: 8px 10px;
		border: none;
		border-bottom: 1px solid color-mix(in oklab, var(--color-base-content) 15%, transparent);
		outline: 0;
		font-size: 0.875rem;
		background: transparent;
		color: var(--color-base-content);
		box-sizing: border-box;
	}
	.select-search::placeholder {
		opacity: 0.4;
	}
	.select-options {
		max-height: 200px;
		overflow-y: auto;
		padding: 4px 0;
	}
	.select-option {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 10px;
		cursor: pointer;
		font-size: 0.875rem;
		color: var(--color-base-content);
		transition: background 0.1s;
	}
	.select-option:hover {
		background: color-mix(in oklab, var(--color-base-content) 8%, transparent);
	}
	.select-option.selected {
		background: color-mix(in oklab, var(--color-primary) 15%, var(--color-base-100));
		color: var(--color-primary);
		font-weight: 500;
	}
	.select-option input[type='checkbox'] {
		accent-color: var(--color-primary);
	}
	.select-no-results {
		display: block;
		padding: 10px;
		text-align: center;
		font-size: 0.8125rem;
		opacity: 0.4;
	}
</style>
