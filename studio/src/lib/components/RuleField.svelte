<script lang="ts">
	import { Lock, Unlock } from '@lucide/svelte';

	let {
		label = 'Rule',
		value = $bindable<string | null>(null),
		name = '',
		placeholder = 'Leave empty to grant everyone access...',
		disabled = false,
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
	class="relative min-h-[62px] rule-field rounded-field bg-base-100 border border-base-300 overflow-hidden"
	class:locked={value === null}
>
	<!-- Label (on top of overlay; clicks pass through only when locked so overlay gets them) -->
	<label for="rule-{name}" class="relative z-[1] flex items-center gap-1 px-3 pt-2 pb-0.5 text-xs font-medium text-base-content/60" class:pointer-events-none={value === null}>
		<span>{label}</span>
		<span
			class="transition-all duration-150"
			class:opacity-100={value === null}
			class:opacity-0={value !== null}
		>(Superusers only)</span>
	</label>

	{#if value === null}
		<!-- Locked state: click anywhere to unlock (label stays above) -->
		<button
			type="button"
			class="group absolute inset-0 z-10 flex items-end justify-end gap-2 pb-2.5 px-3 text-xs font-medium text-success bg-base-200 border-base-300 rounded-field cursor-pointer hover:border-base-400 transition-colors disabled:cursor-not-allowed"
			{disabled}
			onclick={unlock}
		>
			<span class="opacity-0 -translate-x-0.5 transition-all duration-150 group-hover:opacity-100 group-hover:translate-x-0">Unlock and set custom rule</span>
			<Unlock size={18} />
		</button>
	{:else}
		<!-- Unlocked state: input + lock button -->
		<div class="flex items-stretch">
			<div class="flex-1 min-w-0" class:focused>
				<input
					id="rule-{name}"
					type="text"
					class="w-full outline-none border-0 bg-transparent font-mono text-sm px-3 py-2 placeholder:text-base-content/30"
					placeholder={placeholder}
					bind:value
					onfocus={() => focused = true}
					onblur={() => focused = false}
					{disabled}
				/>
			</div>
			<button
				type="button"
				class="flex items-center gap-1 px-2.5 text-xs font-medium text-base-content/60 bg-base-200/60 border border-base-300 border-t-0 border-r-0 cursor-pointer hover:text-success transition-colors disabled:cursor-not-allowed disabled:opacity-50"
				{disabled}
				onclick={lock}
			>
				<Lock size={14} />
				<span class="hidden sm:inline">Set superusers only</span>
			</button>
		</div>
	{/if}
</div>
