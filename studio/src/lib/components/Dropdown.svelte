<script lang="ts">
	import { fly } from 'svelte/transition';
	import { autoUpdate, computePosition, flip, offset, shift } from '@floating-ui/dom';

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
	let menu: HTMLDivElement | undefined = $state();

	// Intro-transition direction (slide out of the trigger's edge). Guessed at
	// open time — the actual placement is floating-ui's job; this only decides
	// which way the 6px slide points, so a rare wrong guess is imperceptible.
	let flyY = $state(-6);

	function toggle() {
		if (!show && wrapper) {
			const wr = wrapper.getBoundingClientRect();
			const viewH = window.innerHeight;
			flyY = wr.bottom + 280 > viewH && wr.top >= 280 ? 6 : -6;
		}
		show = !show;
	}

	// Base style lives outside the positioning updates so the size caps apply
	// from the very first frame (no full-height flash for long option lists).
	// 'position: fixed' anchors the menu to the VIEWPORT, so an
	// overflow:hidden ancestor (e.g. the settings modals) can never clip it.
	const baseStyle = 'max-height:min(320px, 55vh);overflow-y:auto;overflow-x:hidden;';

	// left/top/min-width — filled in by the floating-ui update below.
	let posStyle = $state('');

	$effect(() => {
		if (show && wrapper && menu) {
			// Capture the narrowed references — TS loses the narrowing inside
			// the async callbacks below.
			const ref = wrapper;
			const float = menu;

			// autoUpdate() recomputes on scroll (any scrollable ancestor, capture
			// phase), resize and layout changes, so the menu follows its trigger
			// wherever it scrolls instead of staying glued to the viewport.
			const stopAutoUpdate = autoUpdate(ref, float, () => {
				computePosition(ref, float, {
					strategy: 'fixed',
					placement: align === 'right' ? 'bottom-end' : 'bottom-start',
					// flip() decides above/below from the menu's REAL measured size
					// (the old code guessed 320px and flipped unnecessarily);
					// shift() keeps it inside the viewport horizontally.
					middleware: [offset(4), flip({ padding: 8 }), shift({ padding: 8 })]
				}).then(({ x, y }) => {
					// The menu may have closed between the async call and now.
					if (!show || !float.isConnected) return;
					// At least as wide as the trigger, never narrower than 200px
					// (icon-only triggers), capped so long labels truncate.
					const minW = Math.min(320, Math.max(Math.round(ref.getBoundingClientRect().width), 200));
					posStyle = `left:${Math.round(x)}px;top:${Math.round(y)}px;min-width:${minW}px;`;
				});
			});

			function handle(e: MouseEvent) {
				if (wrapper && !wrapper.contains(e.target as Node)) {
					show = false;
				}
			}
			document.addEventListener('mousedown', handle);

			return () => {
				stopAutoUpdate();
				document.removeEventListener('mousedown', handle);
			};
		} else {
			posStyle = '';
		}
	});
</script>

<div bind:this={wrapper} class="relative {className}">
	<div
		onclick={toggle}
		onkeydown={(e) => {
			if (e.key === 'Enter' || e.key === ' ') {
				e.preventDefault();
				toggle();
			}
		}}
		role="button"
		tabindex="-1"
	>
		{@render trigger?.()}
	</div>

	{#if show}
		<div
			bind:this={menu}
			class="no-scrollbar absolute z-50 rounded-field border-2 border-primary bg-base-100 shadow-lg"
			style={baseStyle + posStyle}
			transition:fly={{ y: flyY, duration: 120 }}
		>
			{@render children?.()}
		</div>
	{/if}
</div>
