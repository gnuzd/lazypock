<script lang="ts">
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
	class="relative min-h-[50px] rule-field"
>
	<label for="rule-{name}" class="flex items-center gap-1 px-3 pt-2 pb-0.5 text-xs font-medium text-base-content/60">
		<span>{label}</span>
		<span
			class="ml-1 transition-all duration-(--animation-speed)"
			class:opacity-100={value !== null}
			class:opacity-0={value === null}
		>(Superusers only)</span>
	</label>

	{#if value === null}
		<!-- Locked state -->
		<button
			type="button"
			class="absolute inset-0 z-10 flex items-center justify-end gap-2 px-3 text-xs font-medium text-success border-2 border-base-300 rounded-field bg-base-200 cursor-pointer hover:border-base-400 transition-colors"
			{disabled}
			onclick={unlock}
		>
			<span class="opacity-0 -translate-x-0.5 group-hover:opacity-100 group-hover:translate-x-0 transition-all">Unlock and set custom rule</span>
			<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/></svg>
		</button>
	{:else}
		<!-- Unlocked state -->
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
				class="flex items-center gap-1 px-2 text-xs font-medium text-base-content/60 border border-base-300 border-t-0 border-r-0 rounded-bl-field rounded-tr-field cursor-pointer hover:text-success transition-colors disabled:cursor-not-allowed disabled:opacity-50"
				{disabled}
				onclick={lock}
			>
				<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="11" width="18" height="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg>
				<span class="hidden sm:inline">Set superusers only</span>
			</button>
		</div>
	{/if}
</div>
