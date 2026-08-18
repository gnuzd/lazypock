<script lang="ts">
	import { fly } from 'svelte/transition';

	let {
		show = $bindable(false),
		class: className = '',
		align = 'left' as 'left' | 'right',
		trigger,
		children
	}: {
		show?: boolean;
		class?: string;
		align?: 'left' | 'right';
		trigger?: import('svelte').Snippet;
		children?: import('svelte').Snippet;
	} = $props();

	let wrapper: HTMLDivElement | undefined = $state();

	// Track where to place the menu. We anchor it to the VIEWPORT
	// (position: fixed) instead of the wrapper so it can never be clipped by
	// an overflow:hidden ancestor (e.g. the settings modals) — the classic
	// "dropdown cut off inside a modal" bug. Width: at least as wide as the
	// trigger, capped so long labels truncate instead of overflowing. Height:
	// capped so long option lists scroll instead of overflowing the viewport.
	let menuStyle = $state('');

	$effect(() => {
		if (show && wrapper) {
			const wr = wrapper.getBoundingClientRect();
			const viewW = window.innerWidth;
			const viewH = window.innerHeight;

			// Menu size caps used both for the flip logic and the CSS.
			const menuW = Math.min(320, Math.max(Math.round(wr.width), 200));
			const menuH = 320;

			// Horizontal: prefer aligning with the requested edge, flip to the
			// other side when that would overflow the viewport.
			let alignLeft = align === 'left';
			if (alignLeft && wr.right + menuW > viewW && wr.left - menuW >= 0) alignLeft = false;
			if (!alignLeft && wr.left - menuW < 0 && wr.right + menuW <= viewW) alignLeft = true;

			// Vertical: show above the trigger when there isn't enough room below.
			const showAbove = wr.bottom + menuH > viewH && wr.top >= menuH;

			const left = alignLeft ? Math.max(4, wr.left) : Math.max(4, wr.right - menuW);
			const top = showAbove ? Math.max(4, wr.top - menuH) : Math.min(viewH - 4, wr.bottom);

			menuStyle =
				`position:fixed;left:${Math.round(left)}px;top:${Math.round(top)}px;` +
				`min-width:${menuW}px;max-width:min(90vw, 360px);` +
				`max-height:min(320px, 55vh);overflow-y:auto;`;

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
	<div onclick={() => (show = !show)} role="button" tabindex="-1">
		{@render trigger?.()}
	</div>

	{#if show}
		<div
			class="z-50 rounded-field border-2 border-primary bg-base-100 shadow-lg"
			style={menuStyle}
			transition:fly={{ y: -6, duration: 120 }}
		>
			{@render children?.()}
		</div>
	{/if}
</div>
