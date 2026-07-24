<script lang="ts">
	import { fly } from 'svelte/transition';

	let {
		show = $bindable(false),
		class: className = '',
		align = 'left' as 'left' | 'right',
		trigger,
		children
	}: {
		show: boolean;
		class?: string;
		align?: 'left' | 'right';
		trigger?: import('svelte').Snippet;
		children?: import('svelte').Snippet;
	} = $props();

	let wrapper: HTMLDivElement | undefined = $state();

	// Track which side to attach the menu for viewport-aware placement
	let menuStyle = $state('');

	$effect(() => {
		if (show && wrapper) {
			const wr = wrapper.getBoundingClientRect();
			const viewW = window.innerWidth;
			const viewH = window.innerHeight;

			// Reserve ~200px for menu width estimate
			const menuW = 200;
			const menuH = 120;

			// Horizontal: flip if overflow
			let left = align === 'left';
			if (align === 'left' && wr.right + menuW > viewW) left = false;
			if (align === 'right' && wr.left - menuW < 0) left = true;

			// Vertical: show above if not enough space below
			const showAbove = wr.bottom + menuH > viewH && wr.top > viewH - wr.bottom;

			let style = '';
			if (showAbove) {
				style += 'bottom:100%;margin-bottom:4px;';
			} else {
				style += 'top:100%;margin-top:4px;';
			}
			if (left) {
				style += 'left:0;right:auto;';
			} else {
				style += 'right:0;left:auto;';
			}
			menuStyle = style;

			function handle(e: MouseEvent) {
				if (wrapper && !wrapper.contains(e.target as Node)) {
					show = false;
				}
			}
			document.addEventListener('mousedown', handle);
			return () => document.removeEventListener('mousedown', handle);
		}
	});
</script>

<div bind:this={wrapper} class="relative {className}">
	<div onclick={() => show = !show} role="button" tabindex="-1">
		{@render trigger?.()}
	</div>

	{#if show}
		<div
			class="absolute z-50 bg-base-100 border-2 border-primary rounded-field shadow-lg"
			style={menuStyle}
			transition:fly={{ y: -6, duration: 120 }}
		>
			{@render children?.()}
		</div>
	{/if}
</div>
