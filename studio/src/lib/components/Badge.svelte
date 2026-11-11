<script module lang="ts">
	import type { Snippet } from 'svelte';

	export type BadgeVariant = 'neutral' | 'primary' | 'info' | 'success' | 'warning' | 'error';

	/** Tinted chip, not a filled one: the colour carries the meaning, not the weight. */
	const VARIANT: Record<BadgeVariant, string> = {
		neutral: 'bg-base-content/10 text-base-content/60',
		primary: 'bg-primary/10 text-primary',
		info: 'bg-info/10 text-info',
		success: 'bg-success/10 text-success',
		warning: 'bg-warning/10 text-warning',
		error: 'bg-error/10 text-error'
	};
</script>

<script lang="ts">
	let {
		variant = 'neutral' as BadgeVariant,
		class: className = '',
		children
	}: { variant?: BadgeVariant; class?: string; children: Snippet } = $props();
</script>

<span
	class="inline-flex items-center rounded-field px-2 py-0.5 text-[0.6875rem] font-medium tracking-wide uppercase {VARIANT[
		variant
	]} {className}"
>
	{@render children()}
</span>
