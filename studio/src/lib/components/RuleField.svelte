<script lang="ts">
	import { Lock, Unlock } from '@lucide/svelte';

	let {
		label = 'Rule',
		value = $bindable<string | null>(null),
		name = '',
		placeholder = 'Leave empty to grant everyone access...',
		disabled = false
	}: {
		label?: string;
		value: string | null;
		name?: string;
		placeholder?: string;
		disabled?: boolean;
	} = $props();

	let prevValue = $state('');
	let focused = $state(false);

	function lock() {
		if (value === null) return;
		prevValue = value;
		value = null;
	}

	function unlock() {
		if (prevValue != null) {
			value = prevValue;
		} else {
			value = '';
		}
		requestAnimationFrame(() => {
			const input = document.getElementById('rule-' + name);
			input?.focus();
		});
	}
</script>

<div
	class="rule-field relative min-h-[62px] overflow-hidden rounded-field border border-base-300 bg-base-100"
	class:locked={value === null}
>
	<!-- Label sits ABOVE the overlay so it's always visible; pointer-events:none passes clicks through -->
	<label
		for="rule-{name}"
		class="relative z-10 flex items-center gap-1 px-3 pt-2 pb-0.5 text-xs font-medium text-base-content/60"
		class:pointer-events-none={value === null}
	>
		<span>{label}</span>
		<span
			class="transition-all duration-150"
			class:opacity-100={value === null}
			class:opacity-0={value !== null}>(Superusers only)</span
		>
	</label>

	{#if value === null}
		<!-- Locked state: click anywhere to unlock (label stays above) -->
		<button
			type="button"
			class="group hover:border-base-400 absolute inset-0 z-0 flex cursor-pointer items-end justify-end gap-2 rounded-field border-base-300 bg-base-200 px-3 pb-2.5 text-xs font-medium text-success transition-colors disabled:cursor-not-allowed"
			{disabled}
			onclick={unlock}
		>
			<span
				class="-translate-x-0.5 opacity-0 transition-all duration-150 group-hover:translate-x-0 group-hover:opacity-100"
				>Unlock and set custom rule</span
			>
			<Unlock size={18} />
		</button>
	{:else}
		<!-- Unlocked state: input + lock button -->
		<div class="flex items-stretch">
			<div class="min-w-0 flex-1" class:focused>
				<input
					id="rule-{name}"
					type="text"
					class="w-full border-0 bg-transparent px-3 py-2 font-mono text-sm outline-none placeholder:text-base-content/30"
					{placeholder}
					bind:value
					onfocus={() => (focused = true)}
					onblur={() => (focused = false)}
					{disabled}
				/>
			</div>
			<button
				type="button"
				class="flex cursor-pointer items-center gap-1 border border-t-0 border-r-0 border-base-300 bg-base-200/60 px-2.5 text-xs font-medium text-base-content/60 transition-colors hover:text-success disabled:cursor-not-allowed disabled:opacity-50"
				{disabled}
				onclick={lock}
			>
				<Lock size={14} />
				<span class="hidden sm:inline">Set superusers only</span>
			</button>
		</div>
	{/if}
</div>
