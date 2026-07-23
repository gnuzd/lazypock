<script lang="ts">
	let {
		fields,
		data = $bindable({}),
		disabled = false
	}: {
		/** Collection schema fields (as returned from backend) */
		fields: Record<string, unknown>[];
		/** Bindable form data — mutated in-place */
		data: Record<string, unknown>;
		/** Disable all inputs (e.g. during save) */
		disabled?: boolean;
	} = $props();

	const RAW_TYPES = new Set([
		'text',
		'number',
		'email',
		'url',
		'date',
		'datetime',
		'editor',
		'password'
	]);

	function isTextInput(f: Record<string, unknown>): boolean {
		return RAW_TYPES.has(f.type as string);
	}

	function getInputType(f: Record<string, unknown>): string {
		const t = f.type as string;
		if (t === 'number') return 'number';
		if (t === 'email') return 'email';
		if (t === 'url') return 'url';
		if (t === 'password') return 'password';
		if (t === 'date' || t === 'datetime') return 'datetime-local';
		return 'text';
	}

	function update(fieldName: string, value: unknown) {
		data[fieldName] = value;
		// Trigger reactivity for $bindable
		data = { ...data };
	}
</script>

<div class="space-y-3">
	{#each fields.filter((f) => !f.system) as field (field.name as string)}
		{@const options = (field.options ?? {}) as Record<string, unknown>}
		{@const type = field.type as string}
		{@const name = field.name as string}
		{@const required = !!field.required}

		{#if type === 'bool'}
			<label class="flex items-center gap-2 cursor-pointer">
				<input
					type="checkbox"
					checked={!!data[name]}
					disabled={disabled}
					onchange={(e) => update(name, (e.target as HTMLInputElement).checked)}
					class="checkbox checkbox-sm"
				/>
				<span class="text-sm text-base-content/80">{name}</span>
			</label>

		{:else if type === 'json' || type === 'geo'}
			<div class="field">
				<label for="f_{name}" class="block text-xs font-medium text-base-content/70 mb-1">
					{name}
					{#if required}<span class="text-error"> *</span>{/if}
				</label>
				<textarea
					id="f_{name}"
					disabled={disabled}
					value={data[name] != null ? JSON.stringify(data[name], null, 2) : ''}
					onchange={(e) => {
						try {
							update(name, JSON.parse((e.target as HTMLTextAreaElement).value));
						} catch {
							// keep existing value if invalid JSON
						}
					}}
					rows="4"
					class="input input-sm w-full font-mono"
					placeholder={'{}'}
				></textarea>
				<p class="text-xs opacity-40 mt-0.5">Valid JSON</p>
			</div>

		{:else if type === 'select'}
			{@const choices = ((options.choices ?? options.values) as string[]) || []}
			<div class="field">
				<label for="f_{name}" class="block text-xs font-medium text-base-content/70 mb-1">
					{name}
					{#if required}<span class="text-error"> *</span>{/if}
				</label>
				<select
					id="f_{name}"
					disabled={disabled}
					value={(data[name] as string) ?? ''}
					onchange={(e) => update(name, (e.target as HTMLSelectElement).value)}
					class="input input-sm w-full"
				>
					<option value="">—</option>
					{#each choices as choice (choice)}
						<option value={choice}>{choice}</option>
					{/each}
				</select>
			</div>

		{:else if type === 'multi_select'}
			{@const choices = ((options.choices ?? options.values) as string[]) || []}
			{@const selected = (data[name] as string[]) || []}
			<div class="field">
				<label class="block text-xs font-medium text-base-content/70 mb-1">
					{name}
					{#if required}<span class="text-error"> *</span>{/if}
				</label>
				<div class="flex flex-wrap gap-1.5">
					{#each choices as choice (choice)}
						<button
							type="button"
							disabled={disabled}
							class="px-2 py-1 text-xs rounded-full border cursor-pointer transition-colors
								{selected.includes(choice)
									? 'bg-primary text-primary-content border-primary'
									: 'bg-base-200 border-base-300 text-base-content/70'}"
							onclick={() => {
								if (selected.includes(choice)) {
									update(name, selected.filter((s) => s !== choice));
								} else {
									update(name, [...selected, choice]);
								}
							}}
						>
							{choice}
						</button>
					{/each}
				</div>
			</div>

		{:else if type === 'file' || type === 'multi_file'}
			<div class="field">
				<label for="f_{name}" class="block text-xs font-medium text-base-content/70 mb-1">
					{name}
					{#if required}<span class="text-error"> *</span>{/if}
				</label>
				<div class="flex items-center gap-2">
					{#if data[name]}
						<span class="text-xs truncate max-w-32 opacity-60">{String(data[name])}</span>
						<button
							type="button"
							disabled={disabled}
							class="text-xs text-error cursor-pointer"
							onclick={() => update(name, null)}
						>
							Remove
						</button>
					{:else}
						<span class="text-xs opacity-40">File upload (drag & drop or click)</span>
					{/if}
				</div>
			</div>

		{:else if type === 'relation'}
			<div class="field">
				<label for="f_{name}" class="block text-xs font-medium text-base-content/70 mb-1">
					{name}
					{#if required}<span class="text-error"> *</span>{/if}
				</label>
				<input
					id="f_{name}"
					type="text"
					disabled={disabled}
					value={(data[name] as string) ?? ''}
					oninput={(e: Event) => update(name, (e.target as HTMLInputElement).value)}
					class="input input-sm w-full"
					placeholder="Related record ID"
				/>
				<p class="text-xs opacity-40 mt-0.5">Related record ID{options.maxSelect && (options.maxSelect as number) > 1 ? 's (comma-separated)' : ''}</p>
			</div>

		{:else if isTextInput(field)}
			{@const isMulti = type === 'editor' || type === 'text'}
			{@const attrs = options as Record<string, unknown>}
			{@const min = attrs.min as number | undefined}
			{@const max = attrs.max as number | undefined}

			{#if isMulti}
				<div class="field">
					<label for="f_{name}" class="block text-xs font-medium text-base-content/70 mb-1">
						{name}
						{#if required}<span class="text-error"> *</span>{/if}
					</label>
					<textarea
						id="f_{name}"
						disabled={disabled}
						value={(data[name] as string) ?? ''}
						oninput={(e: Event) => update(name, (e.target as HTMLTextAreaElement).value)}
						rows="4"
						class="input input-sm w-full"
						placeholder={name}
					></textarea>
				</div>
			{:else}
				<div class="field">
					<label for="f_{name}" class="block text-xs font-medium text-base-content/70 mb-1">
						{name}
						{#if required}<span class="text-error"> *</span>{/if}
					</label>
					<input
						id="f_{name}"
						type={getInputType(field)}
						required={required}
						disabled={disabled}
						value={(data[name] as string) ?? ''}
						oninput={(e: Event) => update(name, (e.target as HTMLInputElement).value)}
						class="input input-sm w-full"
						placeholder={name}
					/>
					{#if min != null || max != null}
						<p class="text-xs opacity-40 mt-0.5">Min: {min ?? '—'}, Max: {max ?? '—'}</p>
					{/if}
				</div>
			{/if}
		{/if}
	{/each}
</div>
