<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import Button from '$lib/components/Button.svelte';
	import Dropdown from '$lib/components/Dropdown.svelte';
	import Input from '$lib/components/Input.svelte';
	import { toast } from 'svelte-sonner';
	import { Check, Plus, X } from '@lucide/svelte';
	import type { FilterString } from 'lazypock';

	// The one "send a test email" control, shared by Mail Settings and the
	// template editor so both queue through the same path: a raw message when no
	// template is named, and `{ to, template, assigns }` when one is. The message
	// is queued, not delivered — a failure shows in the logs, not here.
	//
	// The server takes one address per request, so a list of recipients is a
	// fan-out: the picker and the pasted list both end up as one job each.
	let {
		/** A template to send; omitted → a plain test message. */
		template = '',
		/** The template context. Ignored when `template` is empty. */
		assigns = {},
		/** Panel heading. */
		title = 'Send a test email.',
		/** Panel copy; defaults to a description of whichever mode is in use. */
		description = '',
		/** The input id, so two instances on one page stay labelled separately. */
		id = 'test-recipient'
	}: {
		template?: string;
		assigns?: Record<string, unknown>;
		title?: string;
		description?: string;
		id?: string;
	} = $props();

	const RAW = {
		subject: 'Lazypock test email',
		text: 'This is a test email from your Lazypock server. If you can read it, SMTP is configured correctly.',
		html: '<p>This is a test email from your Lazypock server.</p><p>If you can read it, SMTP is configured correctly.</p>'
	};

	let recipients = $state<string[]>([]);
	/** What is being typed; committed on Enter, on a separator, or on blur. */
	let draft = $state('');
	let sending = $state(false);
	let done = $state(0);
	let total = $state(0);

	/** One queued job per address, in the order they were added. */
	const addresses = $derived([...new Set(recipients)]);

	// The picker: read a collection, show the addresses in one of its fields.
	let collections = $state<string[]>([]);
	let collection = $state('users');
	let fieldNames = $state<string[]>([]);
	let field = $state('email');
	let filter = $state('');
	let candidates = $state<string[]>([]);
	let picked = $state<string[]>([]);
	let totalItems = $state(0); // totalItems of the last candidate query
	let loadingList = $state(false);
	let pickerOpen = $state(false);
	/** The list follows the filter box instead of waiting for a Load click. */
	let filterTimer: ReturnType<typeof setTimeout> | undefined;

	// Loading on open rather than on mount: the picker reads a whole collection, and
	// most visits to this page never open it.
	$effect(() => {
		if (pickerOpen) void loadCollections();
	});

	function scheduleFilter() {
		clearTimeout(filterTimer);
		filterTimer = setTimeout(() => void loadCandidates(), 350);
	}

	const copy = $derived(
		description ||
			(template
				? 'Queues this template to each address below, rendered with the sample values on this page — save first if the change is not stored yet.'
				: 'Uses the saved settings, so save first. The message is queued like any other — a delivery failure shows in the logs rather than here.')
	);

	/** Take a pasted column or a picker selection, both as one comma/space list. */
	function addAddresses(values: string[]) {
		const added = values
			.flatMap((value) => value.split(/[\s,;]+/))
			.map((address) => address.trim())
			.filter(Boolean);
		if (added.length > 0) recipients = [...new Set([...recipients, ...added])];
	}

	function commitDraft() {
		if (!draft.trim()) return;
		addAddresses([draft]);
		draft = '';
	}

	function removeAddress(address: string) {
		recipients = recipients.filter((a) => a !== address);
	}

	function message(err: unknown, fallback: string) {
		const text = (err as { message?: unknown }).message;
		return typeof text === 'string' && text ? text : fallback;
	}

	onMount(async () => {
		try {
			// `requestKey: null`: the page's layout asks for `/me` on the same load and
			// whichever request starts later would abort the other one.
			const me = (await client.me({ requestKey: null })) as Record<string, unknown> | null;
			if (typeof me?.email === 'string') recipients = [me.email];
		} catch {
			// no identity to prefill; the field stays empty
		}
	});

	/** Queue one message for one address; the raw body when no template is named. */
	async function queue(to: string) {
		await client.mail.send(
			template ? { to, template, assigns } : { to, ...RAW },
			// Each address is its own request: without this the SDK's `POST /mail`
			// auto-cancellation key would make the last of a fan-out cancel the rest.
			{ requestKey: null }
		);
	}

	async function sendTestEmail() {
		commitDraft();
		const targets = addresses;
		if (targets.length === 0 || sending) return;
		sending = true;
		total = targets.length;
		done = 0;
		const failures: string[] = [];
		const pending = [...targets];
		// Three at a time: a pasted list of a hundred queues a hundred jobs, but it
		// should not open a hundred requests at once.
		await Promise.all(
			Array.from({ length: Math.min(3, pending.length) }, async () => {
				for (let to = pending.shift(); to !== undefined; to = pending.shift()) {
					try {
						await queue(to);
					} catch (err) {
						failures.push(`${to}: ${message(err, 'failed')}`);
					}
					done += 1;
				}
			})
		);
		sending = false;
		if (failures.length === 0) {
			toast.success(
				`${targets.length} test email${targets.length === 1 ? '' : 's'} queued — check the inbox in a moment.`
			);
		} else if (failures.length === targets.length) {
			toast.error(`Could not queue any of the ${targets.length} test emails. ${failures[0]}`);
		} else {
			toast.warning(
				`Queued ${targets.length - failures.length} of ${targets.length}. ${failures[0]}`
			);
		}
	}

	/**
	 * The collections someone can be in. Only `auth` ones: an address lives on the
	 * user record, so anything else would list rows with nothing to send to.
	 */
	async function loadCollections() {
		if (collections.length === 0) {
			try {
				const res = await client.collections.getList({ page: 1, perPage: 200 });
				collections = ((res?.items ?? []) as unknown as { name: string; type: string }[])
					.filter((c) => c.type === 'auth')
					.map((c) => c.name);
			} catch (err) {
				collections = [];
				toast.error(message(err, 'Could not read the collections'));
			}
		}
		if (!collections.includes(collection)) collection = collections[0] ?? 'users';
		await loadFields();
		await loadCandidates();
	}

	/** The fields that could hold an address, so the picker does not have to guess. */
	async function loadFields() {
		try {
			const coll = (await client.collections.getOne(collection)) as unknown as {
				fields?: { name: string; type: string }[];
			};
			fieldNames = (coll?.fields ?? [])
				.filter((f) => f.type === 'email' || f.type === 'text')
				.map((f) => f.name);
		} catch {
			fieldNames = [];
		}
		if (!fieldNames.includes(field))
			field = fieldNames.find((n) => n === 'email') ?? fieldNames[0] ?? '';
	}

	async function loadCandidates() {
		if (!collection || !field || loadingList) return;
		loadingList = true;
		try {
			const res = await client.collection(collection).getList(1, 50, {
				// The server projects strictly against `fields`, and the Studio's schema
				// snapshot may predate a collection created since it was generated.
				fields: '*',
				...(filter.trim() ? { filter: filter.trim() as FilterString<unknown> } : {})
			});
			totalItems = res?.totalItems ?? 0;
			candidates = [
				...new Set(
					((res?.items ?? []) as Record<string, unknown>[])
						.map((item) => String(item[field] ?? '').trim())
						.filter((value) => value.includes('@'))
				)
			];
		} catch (err) {
			candidates = [];
			totalItems = 0;
			toast.error(message(err, 'Could not read that collection'));
		} finally {
			loadingList = false;
		}
	}

	async function changeCollection() {
		await loadFields();
		await loadCandidates();
	}

	function togglePicked(address: string) {
		picked = picked.includes(address) ? picked.filter((a) => a !== address) : [...picked, address];
	}

	function addPicked() {
		addAddresses(picked);
		picked = [];
	}
</script>

<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<div class="mb-4 text-sm text-base-content/60">
		<p>
			<strong class="text-base-content">{title}</strong>
		</p>
		<p>{copy}</p>
	</div>
	<div class="flex flex-col gap-3">
		<div class="flex items-end gap-2">
			<div class="flex-1">
				<Input
					{id}
					label="Send to"
					placeholder="you@example.com"
					bind:value={draft}
					onkeydown={(e) => {
						if (e.key === 'Enter' || e.key === ',' || e.key === ';') {
							e.preventDefault();
							commitDraft();
						}
					}}
				/>
			</div>
		</div>
		<!-- The addresses land here, under the button that produced them. -->
		{#if addresses.length > 0}
			<div class="mt-2 flex flex-wrap items-center gap-1">
				{#each addresses as address (address)}
					<span
						class="inline-flex items-center gap-1 rounded-full border border-base-300 px-2 py-0.5 font-mono text-xs"
					>
						{address}
						<button
							type="button"
							class="cursor-pointer text-base-content/40 hover:text-base-content"
							aria-label="Remove {address}"
							onclick={() => removeAddress(address)}
						>
							<X size={11} />
						</button>
					</span>
				{/each}
			</div>
		{/if}

		<div>
			<Button
				class="btn-outline"
				loading={sending}
				disabled={sending || (addresses.length === 0 && !draft.trim())}
				onclick={sendTestEmail}
			>
				{sending && total > 1
					? `Sending ${done}/${total}…`
					: `Send test email${addresses.length > 1 ? ` to ${addresses.length}` : ''}`}
			</Button>
		</div>
	</div>
</div>
