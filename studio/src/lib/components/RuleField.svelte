<script lang="ts">
	import { Lock, Unlock } from '@lucide/svelte';
	import { validateRule } from '$lib/ruleValidator';

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
	let dirty = $state(false);
	let validation = $state<{ valid: boolean; error?: string } | null>(null);

	// Re-validate when value changes (only after first interaction)
	$effect(() => {
		const v = value;
		if (v == null) {
			validation = null;
			dirty = false;
		} else if (dirty) {
			validation = validateRule(v);
		}
	});

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
					oninput={() => { if (!dirty) dirty = true; }}
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

		{#if dirty && validation}
			<div class="flex items-center gap-1 px-3 pb-1.5">
				{#if validation.valid}
					<svg
						width="12"
						height="12"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2.5"
						class="shrink-0 text-success"
					><polyline points="20 6 9 17 4 12" /></svg
					>
					<span class="text-xs text-success">Syntax OK</span>
				{:else}
					<svg
						width="12"
						height="12"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						stroke-width="2.5"
						class="shrink-0 text-error"
					><circle cx="12" cy="12" r="10" /><line x1="15" y1="9" x2="9" y2="15" /><line
							x1="9"
							y1="9"
							x2="15"
							y2="15" /></svg
					>
					<span class="text-xs text-error">{validation.error}</span>
				{/if}
			</div>
		{/if}
	{/if}
</div>
