<script lang="ts">
	import { match } from 'ts-pattern';
	import { cn } from '#lib/utils/cn';
	import type { Snippet } from 'svelte';

	type Variant = 'secondary' | 'success' | 'warning' | 'error' | 'default';

	interface Props {
		children?: Snippet;
		variant: Variant;
		class?: string;
	}

	let { children, variant, class: classes }: Props = $props();

	const cls = $derived(
		match(variant)
			.with('secondary', () => 'bg-secondary/15 text-secondary-content border-secondary')
			.with('success', () => 'bg-success/15 text-success-content border-success')
			.with('warning', () => 'bg-warning/15 text-warning-content border-warning')
			.with('error', () => 'bg-error/15 text-error-content border-error')
			.otherwise(() => 'bg-base-300 text-base-content border-base-300')
	);

	const base =
		'inline-flex items-center gap-1.5 rounded-field border px-2 py-0.5 font-mono text-[11px] font-medium {cls}';
</script>

<span class={cn(base, cls, classes)}>
	{#if children}
		{@render children()}
	{/if}
</span>
