<script lang="ts">
	import type { Component } from 'svelte';
	import type { IconProps } from '@lucide/svelte';

	interface Item {
		icon?: Component<IconProps>;
		label: string;
		key: string;
	}

	interface Props {
		items: Array<Item | string>;
		active?: string;
		class?: string;
		onselect?: (item: Item | string) => void;
	}
	let { items = [], active = $bindable(''), class: classes = '', onselect }: Props = $props();

	const onSelect = (item: Item | string) => {
		if (typeof item === 'string') active = item;
		else active = item.key;

		onselect?.(item);
	};
</script>

<nav class="tabs-header {classes}">
	{#each items as item (item)}
		{@const isActive = typeof item === 'string' ? item === active : item.key === active}

		<button class="tab-item" class:active={isActive} onclick={() => onSelect(item)}>
			{#if typeof item !== 'string' && item.icon}
				<item.icon size={14} />
			{/if}

			{#if typeof item === 'string'}
				{item}
			{:else}
				{item.label}
			{/if}
		</button>
	{/each}
</nav>

<style>
	.tabs-header {
		display: flex;
		border-bottom: var(--border, 1px) solid var(--color-base-300);
		gap: 0;
	}

	.tab-item {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: 4px;
		padding: 8px 12px;
		font-size: var(--font-size-sm, 0.875rem);
		font-weight: 500;
		color: var(--color-base-content);
		opacity: 0.6;
		background: none;
		border: none;
		border-bottom: 2px solid transparent;
		cursor: pointer;
		transition:
			opacity var(--animation-speed-fast, 0.1s),
			background var(--animation-speed-fast, 0.1s),
			border-color var(--animation-speed-fast, 0.1s),
			color var(--animation-speed-fast, 0.1s);
		text-align: center;
	}

	.tab-item:hover {
		opacity: 1;
	}

	.tab-item.active {
		opacity: 1;
		border-bottom-color: var(--color-primary);
		color: var(--color-primary);
	}
</style>
