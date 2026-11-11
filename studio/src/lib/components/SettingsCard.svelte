<script lang="ts">
	import type { Component, Snippet } from 'svelte';
	import Badge from './Badge.svelte';
	import type { BadgeVariant } from './Badge.svelte';

	/** A titled box on a settings page: icon, title, category badge, then the fields. */
	let {
		title,
		badge = '',
		badgeVariant = 'neutral' as BadgeVariant,
		icon: Icon,
		children
	}: {
		title: string;
		badge?: string;
		badgeVariant?: BadgeVariant;
		icon: Component<{ size?: number; class?: string }>;
		children: Snippet;
	} = $props();
</script>

<div class="rounded-box border border-base-300 bg-base-100 p-5">
	<div class="flex items-center justify-between gap-3 border-b border-base-300 pb-3">
		<h3 class="flex items-center gap-2 text-sm font-semibold">
			<Icon size={15} class="opacity-60" />
			{title}
		</h3>
		{#if badge}
			<Badge variant={badgeVariant}>{badge}</Badge>
		{/if}
	</div>
	<div class="mt-4 flex flex-col gap-3">
		{@render children()}
	</div>
</div>
