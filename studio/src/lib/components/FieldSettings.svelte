<script lang="ts">
	import { slide } from 'svelte/transition';
	import { slugify } from '$lib/fieldTypes';

	let {
		field = $bindable<Record<string, unknown>>({}),
		fieldIndex = 0,
		collections = [] as Record<string, unknown>[],
	}: {
		field: Record<string, unknown>;
		fieldIndex?: number;
		collections?: Record<string, unknown>[];
	} = $props();

	let open = $state(false);

	function toggle() {
		open = !open;
	}


</script>

{#if !field['@toDelete']}
	<div
		class="border border-base-300 rounded-field bg-base-100 overflow-hidden"
		class:border-primary={open}
		role="region"
		tabindex="-1"
	>
		<!-- Header / summary row -->
		<div class="flex items-center gap-2 p-1">
			<div
				class="flex items-center justify-center w-6 h-full cursor-grab text-base-content/40 hover:text-base-content"
				draggable="true"
			>
				<svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor"><circle cx="5" cy="4" r="1.5"/><circle cx="11" cy="4" r="1.5"/><circle cx="5" cy="8" r="1.5"/><circle cx="11" cy="8" r="1.5"/><circle cx="5" cy="12" r="1.5"/><circle cx="11" cy="12" r="1.5"/></svg>
			</div>

			<!-- Type icon -->
			<div class="flex items-center justify-center w-7 h-7 opacity-60 shrink-0 bg-base-200 rounded">
				{#if field.type === 'text'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="4 7 4 4 20 4 20 7"/><line x1="9" y1="20" x2="15" y2="20"/><line x1="12" y1="4" x2="12" y2="20"/></svg>
				{:else if field.type === 'number'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="4" y1="9" x2="20" y2="9"/><line x1="4" y1="15" x2="20" y2="15"/><line x1="10" y1="3" x2="8" y2="21"/><line x1="16" y1="3" x2="14" y2="21"/></svg>
				{:else if field.type === 'bool'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="2" y="6" width="20" height="12" rx="6"/><circle cx="8" cy="12" r="2"/></svg>
				{:else if field.type === 'email'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z"/><polyline points="22,6 12,13 2,6"/></svg>
				{:else if field.type === 'url'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>
				{:else if field.type === 'date'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="4" width="18" height="18" rx="2" ry="2"/><line x1="16" y1="2" x2="16" y2="6"/><line x1="8" y1="2" x2="8" y2="6"/><line x1="3" y1="10" x2="21" y2="10"/></svg>
				{:else if field.type === 'select'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"/></svg>
				{:else if field.type === 'json'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14.5 3.5L17 6l-2.5 2.5"/><path d="M9.5 3.5L7 6l2.5 2.5"/><path d="M12 20l4-10"/><path d="M4 20h16"/></svg>
				{:else if field.type === 'file'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3z"/><circle cx="12" cy="13" r="3"/></svg>
				{:else if field.type === 'relation'}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="5" y1="12" x2="19" y2="12"/><polyline points="12 5 19 12 12 19"/></svg>
				{:else}
					<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="16" x2="12" y2="12"/><line x1="12" y1="8" x2="12.01" y2="8"/></svg>
				{/if}
			</div>

			<!-- Name input -->
			<div class="flex-1 min-w-0">
				<input
					type="text"
					class="input input-sm w-full bg-transparent border-none text-sm focus:outline-none"
					placeholder="Field name*"
					value={(field.name as string) || ''}
					disabled={!!field.system}
					oninput={(e) => { field.name = slugify((e.target as HTMLInputElement).value); }}
					spellcheck="false"
					required
				/>
			</div>

			<!-- Badges -->
			<div class="flex items-center gap-1 text-[10px] font-medium">
				{#if field.required}
					<span class="px-1 py-0.5 rounded bg-success/20 text-success">Required</span>
				{/if}
				{#if field.hidden}
					<span class="px-1 py-0.5 rounded bg-error/20 text-error">Hidden</span>
				{:else if field.presentable}
					<span class="px-1 py-0.5 rounded bg-info/20 text-info">Presentable</span>
				{/if}
			</div>

			<!-- Header extras -->
			{#if field.type === 'autodate'}
				{@const opt = field.onCreate && field.onUpdate ? 'create/update' : field.onUpdate ? 'update' : 'create'}
				<select
					class="select select-sm max-w-[120px] text-xs"
					value={opt}
					onchange={(e) => {
						const v = (e.target as HTMLSelectElement).value;
						field.onCreate = v === 'create' || v === 'create/update';
						field.onUpdate = v === 'update' || v === 'create/update';
					}}
				>
					<option value="create">Create</option>
					<option value="update">Update</option>
					<option value="create/update">Create/Update</option>
				</select>
			{:else if field.type === 'select'}
					<input
					type="text"
					class="input input-sm max-w-[150px] text-xs"
					placeholder="Add choices*"
					value={(field.values as string[])?.join(' • ') || ''}
					onclick={() => { field._showChoices = true; field._choicesInput = (field.values as string[])?.join('\n') || ''; }}
					readonly
				/>
				{#if field._showChoices}
					<!-- svelte-ignore a11y_click_events_have_key_events -->
					<div class="absolute top-full left-0 z-10 min-w-[260px] bg-base-100 border border-base-300 rounded-field shadow-lg p-2" onclick={(e) => e.stopPropagation()}>
						<div class="text-xs opacity-60 mb-1">New-line separated choices:</div>
						<textarea
							class="input w-full min-h-[60px] text-xs"
							bind:value={field._choicesInput}
							oninput={() => {
								field.values = ((field._choicesInput as string) || '').split('\n').filter((v: string) => v.trim());
								if ((field.maxSelect as number) > (field.values as string[]).length) {
									field.maxSelect = (field.values as string[]).length;
								}
							}}
							onblur={() => { setTimeout(() => field._showChoices = false, 200); }}
						></textarea>
					</div>
				{/if}
				<select class="select select-sm max-w-[80px] text-xs"
					value={(field.maxSelect as number) > 1 ? 'multiple' : 'single'}
					onchange={(e) => { field.maxSelect = (e.target as HTMLSelectElement).value === 'multiple' ? ((field.values as string[])?.length || 2) : 1; }}
				>
					<option value="single">Single</option>
					<option value="multiple">Multiple</option>
				</select>
			{:else if field.type === 'relation'}
				<select class="select select-sm max-w-[150px] text-xs"
					value={(field.collectionId as string) || ''}
					onchange={(e) => field.collectionId = (e.target as HTMLSelectElement).value}
				>
					<option value="" disabled>Select collection*</option>
					{#each collections.filter((c) => (c.type as string) !== 'view') as coll (coll.id as string)}
						<option value={coll.id as string}>{coll.name as string}</option>
					{/each}
				</select>
				<select class="select select-sm max-w-[80px] text-xs"
					value={(field.maxSelect as number) > 1 ? 'multiple' : 'single'}
					onchange={(e) => { field.maxSelect = (e.target as HTMLSelectElement).value === 'multiple' ? 10 : 1; }}
				>
					<option value="single">Single</option>
					<option value="multiple">Multiple</option>
				</select>
			{:else if field.type === 'file'}
				<select class="select select-sm max-w-[80px] text-xs"
					value={(field.maxSelect as number) > 1 ? 'multiple' : 'single'}
					onchange={(e) => { field.maxSelect = (e.target as HTMLSelectElement).value === 'multiple' ? ((field.maxSelect as number) || 10) : 1; }}
				>
					<option value="single">Single</option>
					<option value="multiple">Multiple</option>
				</select>
			{/if}

			<!-- Settings toggle -->
			<button type="button" class="btn btn-ghost btn-sm px-1" title="Field options" onclick={toggle}>
				<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
			</button>
		</div>

		<!-- Expanded settings -->
		{#if open}
			<div class="border-t border-base-300 p-3 space-y-3 text-sm" transition:slide={{ duration: 150 }}>
				{#if field.type === 'text'}
					<div class="grid grid-cols-2 gap-3">
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-min">Min length</label>
							<input type="number" id="field-{fieldIndex}-min" step="1" min="0" placeholder="No min limit" class="input input-sm w-full"
								value={(field.min as number) || ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (v.length > 1 && v[0] == '0') return; field.min = parseInt(v, 10); }}
							/>
						</div>
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-max">Max length</label>
							<input type="number" id="field-{fieldIndex}-max" step="1" min={(field.min as number) || 0} placeholder="Default 5000" class="input input-sm w-full"
								value={(field.max as number) || ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (v.length > 1 && v[0] == '0') return; field.max = parseInt(v, 10); }}
							/>
						</div>
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-pattern">Validation pattern</label>
							<input type="text" id="field-{fieldIndex}-pattern" placeholder="e.g. ^[a-z0-9]+$" class="input input-sm w-full"
								value={(field.pattern as string) || ''}
								oninput={(e) => field.pattern = (e.target as HTMLInputElement).value}
							/>
						</div>
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-autogenerate">Autogenerate pattern</label>
							<input type="text" id="field-{fieldIndex}-autogenerate" placeholder="e.g. [a-z0-9]{30}" class="input input-sm w-full"
								value={(field.autogeneratePattern as string) || ''}
								oninput={(e) => field.autogeneratePattern = (e.target as HTMLInputElement).value}
							/>
						</div>
					</div>
				{:else if field.type === 'number'}
					<div class="grid grid-cols-2 gap-3">
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-min">Min</label>
							<input type="number" id="field-{fieldIndex}-min" class="input input-sm w-full"
								value={typeof field.min === 'number' ? field.min : ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (!v) { field.min = null; return; } field.min = Number(v); }}
							/>
						</div>
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-max">Max</label>
							<input type="number" id="field-{fieldIndex}-max" min={(field.min as number) || undefined} class="input input-sm w-full"
								value={typeof field.max === 'number' ? field.max : ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (!v) { field.max = null; return; } field.max = Number(v); }}
							/>
						</div>
					</div>
				{:else if field.type === 'email'}
					<div class="grid grid-cols-2 gap-3">
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-except">Except domains</label>
							<input type="text" id="field-{fieldIndex}-except" disabled={!!(field.onlyDomains as string[])?.length} class="input input-sm w-full"
								value={(field.exceptDomains as string[])?.join(', ') || ''}
								onchange={(e) => field.exceptDomains = ((e.target as HTMLInputElement).value).split(',').map(s => s.trim()).filter(Boolean)}
							/>
						</div>
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-only">Only domains</label>
							<input type="text" id="field-{fieldIndex}-only" disabled={!!(field.exceptDomains as string[])?.length} class="input input-sm w-full"
								value={(field.onlyDomains as string[])?.join(', ') || ''}
								onchange={(e) => field.onlyDomains = ((e.target as HTMLInputElement).value).split(',').map(s => s.trim()).filter(Boolean)}
							/>
						</div>
					</div>
				{:else if field.type === 'date'}
					<div class="grid grid-cols-2 gap-3">
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-minDate">Min date</label>
							<input type="datetime-local" id="field-{fieldIndex}-minDate" step="1" class="input input-sm w-full"
								value={(field.min as string) || ''}
								onchange={(e) => field.min = (e.target as HTMLInputElement).value}
							/>
						</div>
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxDate">Max date</label>
							<input type="datetime-local" id="field-{fieldIndex}-maxDate" step="1" class="input input-sm w-full"
								value={(field.max as string) || ''}
								onchange={(e) => field.max = (e.target as HTMLInputElement).value}
							/>
						</div>
					</div>
				{:else if field.type === 'select'}
					<div class="field" hidden={(field.maxSelect as number) < 2}>
						<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxSelect">Max select</label>
						<input type="number" id="field-{fieldIndex}-maxSelect" step="1" min="2" max={(field.values as string[])?.length || 2} placeholder="Default to single" class="input input-sm w-full"
							value={(field.maxSelect as number) || ''}
							onchange={(e) => { const v = parseInt((e.target as HTMLInputElement).value, 10); field.maxSelect = v > 1 ? v : 1; }}
						/>
					</div>
				{:else if field.type === 'json'}
					<div class="field">
						<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxSize">Max size (bytes)</label>
						<input type="number" id="field-{fieldIndex}-maxSize" step="1" min="0" placeholder="Default ~1MB" class="input input-sm w-full"
							value={(field.maxSize as number) || ''}
							oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (v.length > 1 && v[0] == '0') return; field.maxSize = parseInt(v, 10); }}
						/>
					</div>
				{:else if field.type === 'file'}
					<div class="grid grid-cols-2 gap-3">
						<div class="field col-span-2">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-mimeTypes">Allowed mime types</label>
							<input type="text" id="field-{fieldIndex}-mimeTypes" placeholder="No restriction" class="input input-sm w-full"
								value={(field.mimeTypes as string[])?.join(', ') || ''}
								onchange={(e) => field.mimeTypes = ((e.target as HTMLInputElement).value).split(',').map(s => s.trim()).filter(Boolean)}
							/>
						</div>
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-thumbs">Thumb sizes</label>
							<input type="text" id="field-{fieldIndex}-thumbs" placeholder="e.g. 50x50, 480x720" class="input input-sm w-full"
								value={(field.thumbs as string[])?.join(', ') || ''}
								onchange={(e) => field.thumbs = ((e.target as HTMLInputElement).value).split(',').map(s => s.trim()).filter(Boolean)}
							/>
						</div>
						<div class="field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxSize">Max size (bytes)</label>
							<input type="number" id="field-{fieldIndex}-maxSize" step="1" min="0" placeholder="~5MB default" class="input input-sm w-full"
								value={(field.maxSize as number) || ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (v.length > 1 && v[0] == '0') return; field.maxSize = parseInt(v, 10); }}
							/>
						</div>
						<div class="field flex items-center gap-2 mt-1">
							<input type="checkbox" class="checkbox checkbox-sm" id="field-{fieldIndex}-protected"
								checked={!!field.protected} onchange={(e) => field.protected = (e.target as HTMLInputElement).checked}
							/>
							<label class="text-xs cursor-pointer" for="field-{fieldIndex}-protected">Protected</label>
						</div>
					</div>
				{:else if field.type === 'relation'}
					<div class="field" hidden={(field.maxSelect as number) < 2}>
						<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-minSelect">Min select</label>
						<input type="number" id="field-{fieldIndex}-minSelect" step="1" min="0" placeholder="No min limit" class="input input-sm w-full"
							value={(field.minSelect as number) || ''}
							onchange={(e) => field.minSelect = parseInt((e.target as HTMLInputElement).value, 10)}
						/>
					</div>
					<div class="field" hidden={(field.maxSelect as number) < 2}>
						<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxSelect">Max select</label>
						<input type="number" id="field-{fieldIndex}-maxSelect" step="1" min={(field.minSelect as number) || 2} placeholder="Default to single" class="input input-sm w-full"
							value={(field.maxSelect as number) || ''}
							onchange={(e) => { const v = parseInt((e.target as HTMLInputElement).value, 10); field.maxSelect = v > 1 ? v : 1; }}
						/>
					</div>
					<div class="field">
						<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-cascade">Cascade delete</label>
						<select class="select select-sm w-full" id="field-{fieldIndex}-cascade"
							value={field.cascadeDelete ? 'true' : 'false'}
							onchange={(e) => field.cascadeDelete = (e.target as HTMLSelectElement).value === 'true'}
						>
							<option value="false">False</option>
							<option value="true">True</option>
						</select>
					</div>
				{/if}

				<!-- Help text for all types -->
				<div class="field">
					<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-help">Help text</label>
					<input type="text" id="field-{fieldIndex}-help" class="input input-sm w-full"
						value={(field.help as string) || ''}
						oninput={(e) => field.help = (e.target as HTMLInputElement).value}
					/>
				</div>
			</div>

			<!-- Footer (type-specific options + delete) -->
			<div class="flex items-center gap-2 px-3 py-2 bg-base-200 border-t border-base-300 text-xs">
				{#if field.type === 'number'}
					<label class="flex items-center gap-1 cursor-pointer">
						<input type="checkbox" class="checkbox checkbox-sm" checked={!!field.onlyInt} onchange={(e) => field.onlyInt = (e.target as HTMLInputElement).checked} />
						No decimals
					</label>
				{/if}
				{#if field.type === 'text' || field.type === 'email' || field.type === 'url' || field.type === 'select' || field.type === 'relation' || field.type === 'editor' || field.type === 'password'}
					<label class="flex items-center gap-1 cursor-pointer">
						<input type="checkbox" class="checkbox checkbox-sm" id="field-{fieldIndex}-required" checked={!!field.required} onchange={(e) => field.required = (e.target as HTMLInputElement).checked} />
						Required
					</label>
				{/if}
				<label class="flex items-center gap-1 cursor-pointer">
					<input type="checkbox" class="checkbox checkbox-sm" id="field-{fieldIndex}-presentable" checked={!!field.presentable} onchange={(e) => field.presentable = (e.target as HTMLInputElement).checked} disabled={!!field.hidden} />
					Presentable
				</label>
				<label class="flex items-center gap-1 cursor-pointer">
					<input type="checkbox" class="checkbox checkbox-sm" id="field-{fieldIndex}-hidden" checked={!!field.hidden} onchange={(e) => field.hidden = (e.target as HTMLInputElement).checked} />
					Hidden
				</label>
				<div class="ml-auto">
									<button type="button" class="btn btn-ghost btn-sm text-error px-1" title="Delete field" onclick={() => field['@toDelete'] = true}>
						<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
					</button>
				</div>
			</div>
		{/if}
	</div>
{/if}
