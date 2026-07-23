<script lang="ts">
	import { slide } from 'svelte/transition';
	import {
		FileText, FileCode, Hash, ToggleLeft, Mail, Link, Calendar,
		CalendarCheck, ListChecks, Braces, Image, GitBranch, MapPin, Lock,
		Settings, Trash2, GripVertical
	} from '@lucide/svelte';
	import { slugify } from '$lib/fieldTypes';

	const typeIcons: Record<string, typeof FileText> = {
		text: FileText,
		editor: FileCode,
		number: Hash,
		bool: ToggleLeft,
		email: Mail,
		url: Link,
		date: Calendar,
		autodate: CalendarCheck,
		select: ListChecks,
		json: Braces,
		file: Image,
		relation: GitBranch,
		geoPoint: MapPin,
		password: Lock,
	};

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
		<div class="flex items-center gap-2 p-1.5">
			<div
				class="flex items-center justify-center w-6 h-full cursor-grab text-base-content/40 hover:text-base-content"
				draggable="true"
			>
				<GripVertical size={16} />
			</div>

			<!-- Type icon -->
			<div class="flex items-center justify-center w-7 h-7 opacity-60 shrink-0 bg-base-200 rounded">
				{#each [typeIcons[field.type as string] || FileText] as Cmp (Cmp)}
					<Cmp size={16} />
				{/each}
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
				<Settings size={16} />
			</button>
		</div>

		<!-- Expanded settings -->
		{#if open}
			<div class="border-t border-base-300 p-3 space-y-3 text-sm" transition:slide={{ duration: 150 }}>
				{#if field.type === 'text'}
					<div class="grid grid-cols-2 gap-3">
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-min">Min length</label>
							<input type="number" id="field-{fieldIndex}-min" step="1" min="0" placeholder="No min limit" class="input input-sm w-full"
								value={(field.min as number) || ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (v.length > 1 && v[0] == '0') return; field.min = parseInt(v, 10); }}
							/>
						</div>
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-max">Max length</label>
							<input type="number" id="field-{fieldIndex}-max" step="1" min={(field.min as number) || 0} placeholder="Default 5000" class="input input-sm w-full"
								value={(field.max as number) || ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (v.length > 1 && v[0] == '0') return; field.max = parseInt(v, 10); }}
							/>
						</div>
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-pattern">Validation pattern</label>
							<input type="text" id="field-{fieldIndex}-pattern" placeholder="e.g. ^[a-z0-9]+$" class="input input-sm w-full"
								value={(field.pattern as string) || ''}
								oninput={(e) => field.pattern = (e.target as HTMLInputElement).value}
							/>
						</div>
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-autogenerate">Autogenerate pattern</label>
							<input type="text" id="field-{fieldIndex}-autogenerate" placeholder="e.g. [a-z0-9]{30}" class="input input-sm w-full"
								value={(field.autogeneratePattern as string) || ''}
								oninput={(e) => field.autogeneratePattern = (e.target as HTMLInputElement).value}
							/>
						</div>
					</div>
				{:else if field.type === 'number'}
					<div class="grid grid-cols-2 gap-3">
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-min">Min</label>
							<input type="number" id="field-{fieldIndex}-min" class="input input-sm w-full"
								value={typeof field.min === 'number' ? field.min : ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (!v) { field.min = null; return; } field.min = Number(v); }}
							/>
						</div>
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-max">Max</label>
							<input type="number" id="field-{fieldIndex}-max" min={(field.min as number) || undefined} class="input input-sm w-full"
								value={typeof field.max === 'number' ? field.max : ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (!v) { field.max = null; return; } field.max = Number(v); }}
							/>
						</div>
					</div>
				{:else if field.type === 'email'}
					<div class="grid grid-cols-2 gap-3">
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-except">Except domains</label>
							<input type="text" id="field-{fieldIndex}-except" disabled={!!(field.onlyDomains as string[])?.length} class="input input-sm w-full"
								value={(field.exceptDomains as string[])?.join(', ') || ''}
								onchange={(e) => field.exceptDomains = ((e.target as HTMLInputElement).value).split(',').map(s => s.trim()).filter(Boolean)}
							/>
						</div>
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-only">Only domains</label>
							<input type="text" id="field-{fieldIndex}-only" disabled={!!(field.exceptDomains as string[])?.length} class="input input-sm w-full"
								value={(field.onlyDomains as string[])?.join(', ') || ''}
								onchange={(e) => field.onlyDomains = ((e.target as HTMLInputElement).value).split(',').map(s => s.trim()).filter(Boolean)}
							/>
						</div>
					</div>
				{:else if field.type === 'date'}
					<div class="grid grid-cols-2 gap-3">
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-minDate">Min date</label>
							<input type="datetime-local" id="field-{fieldIndex}-minDate" step="1" class="input input-sm w-full"
								value={(field.min as string) || ''}
								onchange={(e) => field.min = (e.target as HTMLInputElement).value}
							/>
						</div>
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxDate">Max date</label>
							<input type="datetime-local" id="field-{fieldIndex}-maxDate" step="1" class="input input-sm w-full"
								value={(field.max as string) || ''}
								onchange={(e) => field.max = (e.target as HTMLInputElement).value}
							/>
						</div>
					</div>
				{:else if field.type === 'select'}
					<div class="bg-base-200/40 p-1.5 rounded-field" hidden={(field.maxSelect as number) < 2}>
						<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxSelect">Max select</label>
						<input type="number" id="field-{fieldIndex}-maxSelect" step="1" min="2" max={(field.values as string[])?.length || 2} placeholder="Default to single" class="input input-sm w-full"
							value={(field.maxSelect as number) || ''}
							onchange={(e) => { const v = parseInt((e.target as HTMLInputElement).value, 10); field.maxSelect = v > 1 ? v : 1; }}
						/>
					</div>
				{:else if field.type === 'json'}
					<div class="bg-base-200/40 p-1.5 rounded-field">
						<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxSize">Max size (bytes)</label>
						<input type="number" id="field-{fieldIndex}-maxSize" step="1" min="0" placeholder="Default ~1MB" class="input input-sm w-full"
							value={(field.maxSize as number) || ''}
							oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (v.length > 1 && v[0] == '0') return; field.maxSize = parseInt(v, 10); }}
						/>
					</div>
				{:else if field.type === 'file'}
					<div class="grid grid-cols-2 gap-3">
						<div class="col-span-2 bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-mimeTypes">Allowed mime types</label>
							<input type="text" id="field-{fieldIndex}-mimeTypes" placeholder="No restriction" class="input input-sm w-full"
								value={(field.mimeTypes as string[])?.join(', ') || ''}
								onchange={(e) => field.mimeTypes = ((e.target as HTMLInputElement).value).split(',').map(s => s.trim()).filter(Boolean)}
							/>
						</div>
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-thumbs">Thumb sizes</label>
							<input type="text" id="field-{fieldIndex}-thumbs" placeholder="e.g. 50x50, 480x720" class="input input-sm w-full"
								value={(field.thumbs as string[])?.join(', ') || ''}
								onchange={(e) => field.thumbs = ((e.target as HTMLInputElement).value).split(',').map(s => s.trim()).filter(Boolean)}
							/>
						</div>
						<div class="bg-base-200/40 p-1.5 rounded-field">
							<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxSize">Max size (bytes)</label>
							<input type="number" id="field-{fieldIndex}-maxSize" step="1" min="0" placeholder="~5MB default" class="input input-sm w-full"
								value={(field.maxSize as number) || ''}
								oninput={(e) => { const v = (e.target as HTMLInputElement).value; if (v.length > 1 && v[0] == '0') return; field.maxSize = parseInt(v, 10); }}
							/>
						</div>
						<div class="bg-base-200/40 p-1.5 rounded-field flex items-center gap-2 mt-1">
							<input type="checkbox" class="checkbox checkbox-sm" id="field-{fieldIndex}-protected"
								checked={!!field.protected} onchange={(e) => field.protected = (e.target as HTMLInputElement).checked}
							/>
							<label class="text-xs cursor-pointer" for="field-{fieldIndex}-protected">Protected</label>
						</div>
					</div>
				{:else if field.type === 'relation'}
					<div class="bg-base-200/40 p-1.5 rounded-field" hidden={(field.maxSelect as number) < 2}>
						<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-minSelect">Min select</label>
						<input type="number" id="field-{fieldIndex}-minSelect" step="1" min="0" placeholder="No min limit" class="input input-sm w-full"
							value={(field.minSelect as number) || ''}
							onchange={(e) => field.minSelect = parseInt((e.target as HTMLInputElement).value, 10)}
						/>
					</div>
					<div class="bg-base-200/40 p-1.5 rounded-field" hidden={(field.maxSelect as number) < 2}>
						<label class="text-xs font-medium text-base-content/70 block mb-1" for="field-{fieldIndex}-maxSelect">Max select</label>
						<input type="number" id="field-{fieldIndex}-maxSelect" step="1" min={(field.minSelect as number) || 2} placeholder="Default to single" class="input input-sm w-full"
							value={(field.maxSelect as number) || ''}
							onchange={(e) => { const v = parseInt((e.target as HTMLInputElement).value, 10); field.maxSelect = v > 1 ? v : 1; }}
						/>
					</div>
					<div class="bg-base-200/40 p-1.5 rounded-field">
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
				<div class="bg-base-200/40 p-1.5 rounded-field">
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
						<Trash2 size={14} />
					</button>
				</div>
			</div>
		{/if}
	</div>
{/if}
