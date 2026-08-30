<script lang="ts">
	import Erd from '$lib/components/Erd.svelte';
	import { collections } from '$lib/collectionsStore';

	let {
		show = $bindable(false)
	}: {
		show?: boolean;
	} = $props();

	let activeTab = $state('Fields and relations');
	let showSystem = $state(false);

	const tabs = ['Fields and relations', 'Rules'];

	/** PB's overview filters system collections by default (toggle available). */
	const visibleCollections = $derived.by(() => {
		const list = $collections;
		if (showSystem) return list;
		return list.filter((c) => !c.system);
	});

	const ruleOptions = $derived.by(() => {
		const base = [
			{ value: 'listRule', label: 'List/Search rule', filter: () => true },
			{ value: 'viewRule', label: 'View rule', filter: () => true },
			{ value: 'createRule', label: 'Create rule', filter: (c: Record<string, unknown>) => c.type !== 'view' },
			{ value: 'updateRule', label: 'Update rule', filter: (c: Record<string, unknown>) => c.type !== 'view' },
			{ value: 'deleteRule', label: 'Delete rule', filter: (c: Record<string, unknown>) => c.type !== 'view' }
		];
		if (visibleCollections.some((c) => c.type === 'auth')) {
			base.push({ value: 'manageRule', label: 'Manage rule', filter: (c: Record<string, unknown>) => c.type === 'auth' });
		}
		return base;
	});

	let activeRule = $state('listRule');

	function ruleDisplay(collection: Record<string, unknown>, ruleKey: string): {
		kind: 'superusers' | 'public' | 'code';
		value?: string;
	} {
		const rules = (collection.rules as Record<string, unknown>) ?? {};
		const value = rules[ruleKey];
		if (value === null || value === undefined) return { kind: 'superusers' };
		if (value === '') return { kind: 'public' };
		return { kind: 'code', value: String(value) };
	}
</script>

{#if show}
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<div
		class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-6"
		role="dialog"
		aria-modal="true"
		tabindex="-1"
		onclick={() => (show = false)}
	>
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<div
			class="flex max-h-[85vh] w-full max-w-5xl flex-col overflow-hidden rounded-box bg-base-100 shadow-xl"
			onclick={(e) => e.stopPropagation()}
		>
			<header class="flex shrink-0 items-center gap-3 border-b border-base-300 px-5 py-3">
				<h2 class="text-lg font-semibold">Collections overview</h2>
				<div class="ml-auto flex items-center gap-3">
					<label class="flex cursor-pointer items-center gap-2 text-sm text-base-content/70">
						<input type="checkbox" class="checkbox checkbox-sm" bind:checked={showSystem} />
						System collections
					</label>
					<button type="button" class="btn btn-ghost btn-sm" onclick={() => (show = false)}>
						Close
					</button>
				</div>
			</header>

			<nav class="flex shrink-0 gap-0 border-b border-base-300 px-4">
				{#each tabs as tab (tab)}
					<button
						type="button"
						class="cursor-pointer border-b-2 px-3 py-2 text-sm transition-colors {activeTab ===
						tab
							? 'border-primary text-primary'
							: 'border-transparent text-base-content/60'}"
						onclick={() => (activeTab = tab)}
					>
						{tab}
					</button>
				{/each}
			</nav>

			<div
				class="min-h-0 flex-1 overflow-auto p-4 {activeTab === 'Fields and relations' ? 'flex flex-col' : ''}"
			>
				{#if activeTab === 'Fields and relations'}
					<div class="relative min-h-[420px] flex-1">
						<Erd collections={visibleCollections} />
					</div>
				{:else}
					<!-- Rules overview (PocketBase parity) -->
					<div class="flex flex-col gap-3">
						<div class="flex flex-wrap gap-1.5">
							{#each ruleOptions as opt (opt.value)}
								<button
									type="button"
									class="btn btn-sm {activeRule === opt.value ? 'btn-outline' : 'btn-ghost'}"
									onclick={() => (activeRule = opt.value)}
								>
									{opt.label}
								</button>
							{/each}
						</div>

						<div class="overflow-x-auto rounded-field border border-base-300">
							<table class="w-full text-sm">
								<tbody>
									{#each visibleCollections as collection (collection.id as string)}
										{@const ruleKey = activeRule}
										{@const display = ruleDisplay(collection, ruleKey)}
										{#if (ruleOptions.find((o) => o.value === ruleKey)?.filter(collection) ?? true)}
											<tr class="border-b border-base-300 last:border-b-0">
												<td class="w-48 px-4 py-2 align-top">
													<span class="font-medium">{collection.name as string}</span>
													<span class="ml-1 text-xs opacity-40">({collection.type as string})</span>
												</td>
												<td class="px-4 py-2 align-top">
													{#if display.kind === 'superusers'}
														<span class="rounded bg-base-300 px-1.5 py-0.5 text-xs">Superusers only</span>
													{:else if display.kind === 'public'}
														<span class="rounded bg-success/20 px-1.5 py-0.5 text-xs text-success"
															>Public</span
														>
													{:else}
														<code class="block rounded bg-base-200 px-2 py-1 font-mono text-xs"
															>{display.value}</code
														>
													{/if}
												</td>
											</tr>
										{/if}
									{/each}
								</tbody>
							</table>
						</div>

						{#if !visibleCollections.some((c) => ruleOptions.find((o) => o.value === activeRule)?.filter(c))}
							<p class="text-sm text-base-content/50">No collections with the selected rule.</p>
						{/if}
					</div>
				{/if}
			</div>
		</div>
	</div>
{/if}
