<script lang="ts">
	import { client } from '$lib/client';
	import { confirmDialog, promptDialog } from '$lib/dialog.svelte';
	import type { MailTemplate, MailTemplateLayer } from 'lazypock';
	import { onMount, tick } from 'svelte';
	import { Plus, Undo2 } from '@lucide/svelte';
	import Button from '$lib/components/Button.svelte';
	import MailTestSend from '$lib/components/MailTestSend.svelte';
	import RichEditor from '$lib/components/RichEditor.svelte';
	import Tabs from '$lib/components/Tabs.svelte';
	import { toast } from 'svelte-sonner';
	import '../settings.css';

	// The template editor: the three parts of every email the server can send,
	// with the layer each one currently resolves from and a renderer to see the
	// result before anyone receives it.
	//
	// Three deliberate choices:
	//
	// * A template is saved as a whole (`PUT`), because a template only exists
	//   once it is complete — a part-at-a-time write could leave a subject saved
	//   against a body that does not parse.
	// * The preview is a real render by the server, in a **sandboxed** iframe
	//   (`sandbox=""`, so no scripts and no access to the Studio's origin) — an
	//   email body is untrusted input, never `{@html}` in this document.
	// * Reset deletes the stored bodies, which is destructive enough to ask first;
	//   it falls back to a file in the template directory, then to the built-in.

	type Layer = MailTemplateLayer;

	const PARTS = ['subject', 'html', 'text'] as const;
	type Part = (typeof PARTS)[number];

	const LAYER_HELP: Record<Layer, string> = {
		database: 'Saved here, and the one in effect.',
		file: 'From a file in the deployment’s template directory.',
		builtin: 'Shipped with the server.'
	};

	// Held in constants because Svelte reads a literal `{{ … }}` in an attribute as
	// an object expression rather than as text.
	const PLACEHOLDER_SUBJECT = 'Your receipt from {{ app_name }}';
	const PLACEHOLDER_HTML = '<p>Hello {{ name }},</p>';
	const PLACEHOLDER_TEXT = 'Hello {{ name }},';

	let templates = $state<MailTemplate[]>([]);
	let loading = $state(true);
	/** The name being edited — a stored one, or a new one that is not saved yet. */
	let selected = $state('');
	/** True while the selection is a name nothing has stored yet. */
	let creating = $state(false);
	let draft = $state<Record<Part, string>>({ subject: '', html: '', text: '' });
	/** The sample values handed to `{{ … }}`, as JSON. */
	let sample = $state('{}');
	let saving = $state(false);
	let resetting = $state(false);
	let previewTab = $state('HTML');
	let rendered = $state<{ subject: string; html: string; text: string } | null>(null);
	let renderError = $state('');
	let previewing = $state(false);
	/** How the HTML body is being edited: the rendered document, or its source. */
	let htmlMode = $state<'rich' | 'html'>('rich');
	/** The rich editor, so `{x}` can insert a variable at its cursor. */
	let richBody = $state<{ insertText: (text: string) => void } | null>(null);
	/** The source textarea, for the same reason while the raw body is being edited. */
	let htmlArea = $state<HTMLTextAreaElement | null>(null);
	/** The variables the last preview left empty, because the sample does not supply them. */
	let missing = $state<string[]>([]);
	/** Who a test send would reach, and who the server sends as. */
	let meEmail = $state('');
	let sender = $state('');

	/** The stored template behind the editor, when there is one. */
	const current = $derived(templates.find((t) => t.name === selected) ?? null);

	const parsedSample = $derived.by((): { value: Record<string, unknown>; error: string } => {
		const raw = sample.trim();
		if (!raw) return { value: {}, error: '' };
		try {
			const parsed: unknown = JSON.parse(raw);
			if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
				return { value: {}, error: 'The sample must be a JSON object.' };
			}
			return { value: parsed as Record<string, unknown>, error: '' };
		} catch {
			return { value: {}, error: 'The sample is not valid JSON.' };
		}
	});

	const dirty = $derived(
		creating ||
			(current !== null && PARTS.some((part) => draft[part] !== (current as MailTemplate)[part]))
	);
	const complete = $derived(PARTS.every((part) => draft[part].trim() !== ''));
	/** A template can only be reset once something of it is stored. */
	const resettable = $derived(!creating && current !== null && current.source === 'database');
	const sourceNotice = $derived(
		creating
			? 'A new template — all three parts are needed before it can be saved.'
			: `Comes from ${current?.source ?? '—'}.`
	);

	/** The `{{ ... }}` names the draft uses, for the variable menu. */
	const variables = $derived.by(() => {
		const names = new Set<string>();
		for (const part of PARTS) {
			for (const [, name] of draft[part].matchAll(/\{\{\s*([A-Za-z_][A-Za-z0-9_.]*)/g)) {
				names.add(name);
			}
		}
		// `name` is filled in by the server whatever the sample says, so the menu always
		// has something to offer a body that uses no variables yet.
		return [...(names.size ? names : new Set(['name']))].sort();
	});

	/** Insert a variable into the HTML body, wherever it is being edited. */
	function insertVariable(name: string) {
		const token = `{{ ${name} }}`;
		if (htmlMode === 'rich') {
			richBody?.insertText(token);
			return;
		}
		insertIntoArea(htmlArea, 'html', token);
	}

	/** Insert `text` at the cursor of a textarea and put the caret after it. */
	function insertIntoArea(area: HTMLTextAreaElement | null, part: Part, text: string) {
		if (!area) return;
		const { selectionStart: start, selectionEnd: end } = area;
		draft[part] = draft[part].slice(0, start) + text + draft[part].slice(end);
		const caret = start + text.length;
		// The value update above re-renders the textarea, so the caret goes on after it.
		void tick().then(() => {
			area.focus();
			area.setSelectionRange(caret, caret);
		});
	}

	function message(err: unknown, fallback: string): string {
		const text = (err as { message?: unknown }).message;
		return typeof text === 'string' && text ? text : fallback;
	}

	async function load() {
		loading = true;
		try {
			templates = await client.mail.listTemplates();
		} catch (err) {
			templates = [];
			toast.error(message(err, 'Could not load the mail templates'));
		} finally {
			loading = false;
		}
	}

	/**
	 * What the frame shows above the body: the sample's own address when it names
	 * one (email_change is the template that does), otherwise the signed-in
	 * superuser — the address a test send starts from.
	 */
	const toAddress = $derived(
		typeof parsedSample.value.email === 'string' && parsedSample.value.email
			? parsedSample.value.email
			: meEmail
	);

	onMount(async () => {
		await load();
		const first = templates[0];
		if (first && !selected) select(first.name);
		// Only for the frame around the preview; a failure leaves one row blank.
		//
		// `requestKey: null` on both: the SDK auto-cancels a request with the same
		// method and path, and the layout (plus MailTestSend) asks for `/me` on the
		// very same page load — one of the three would abort with no visible cause.
		try {
			const me = (await client.me({ requestKey: null })) as Record<string, unknown> | null;
			meEmail = (me?.email as string) ?? '';
		} catch {
			// the frame shows a dash
		}
		try {
			const settings = (await client.http.get('/settings', {
				requestKey: null
			})) as Record<string, unknown> | null;
			const mail = (settings?.mail ?? {}) as Record<string, unknown>;
			const name = (mail.sender_name as string) ?? '';
			const address = (mail.sender_address as string) ?? '';
			sender = address ? (name ? `${name} <${address}>` : address) : '';
		} catch {
			// the frame shows a dash
		}
	});

	function select(name: string) {
		const template = templates.find((t) => t.name === name);
		if (!template) return;
		selected = name;
		creating = false;
		rendered = null;
		renderError = '';
		missing = [];
		draft = { subject: template.subject, html: template.html, text: template.text };
	}

	/** Switch templates, refusing to throw away edits without a word. */
	async function choose(name: string) {
		if (name === selected) return;
		if (dirty) {
			const discard = await confirmDialog({
				title: 'Discard changes',
				message: `Discard your unsaved changes to "${selected}"?`,
				confirmLabel: 'Discard',
				variant: 'error'
			});
			if (!discard) return;
		}
		select(name);
	}

	async function startNew() {
		if (dirty && selected) {
			const discard = await confirmDialog({
				title: 'Discard changes',
				message: `Discard your unsaved changes to "${selected}"?`,
				confirmLabel: 'Discard',
				variant: 'error'
			});
			if (!discard) return;
		}
		const name = await promptDialog({
			title: 'New template',
			message:
				'A name for the template — no spaces. It is stored once all three parts are filled in.',
			placeholder: 'e.g. receipt',
			confirmLabel: 'Start'
		});
		if (name === null) return;
		const trimmed = name.trim();
		if (!trimmed) return;
		if (/\s/.test(trimmed)) {
			toast.error('A template name cannot contain spaces.');
			return;
		}
		if (templates.some((t) => t.name === trimmed)) {
			toast.error(`A template named "${trimmed}" already exists.`);
			select(trimmed);
			return;
		}
		selected = trimmed;
		creating = true;
		rendered = null;
		renderError = '';
		missing = [];
		draft = { subject: '', html: '', text: '' };
	}

	async function save() {
		if (!selected || saving || !complete) return;
		saving = true;
		try {
			await client.mail.saveTemplate(selected, { ...draft });
			toast.success(`Saved the ${selected} template.`);
			creating = false;
			// Reload rather than patching the list from the response: a new name is not in
			// `templates` yet, and the reload is also what reports the layer each part now
			// resolves from. `select()` is deliberately not called — it would blank the
			// preview, which is already the render of what was just stored.
			await load();
		} catch (err) {
			// A body that does not parse is refused at save time rather than
			// queueing an email that would fail where nobody is watching.
			toast.error(message(err, 'Could not save the template'));
		} finally {
			saving = false;
		}
	}

	async function reset() {
		if (!selected || resetting || !resettable) return;
		const stored = PARTS.filter((part) => current?.parts[part] === 'database');
		const confirmed = await confirmDialog({
			title: 'Reset template',
			message: `Reset "${selected}" to the file or built-in copy?`,
			details: [
				`Deletes the stored ${stored.join(', ')} part${stored.length === 1 ? '' : 's'}.`,
				'The mail then renders from the file in the template directory, or the built-in.',
				'If nothing else defines this name, the template is removed.'
			],
			confirmLabel: 'Reset',
			variant: 'error'
		});
		if (!confirmed) return;
		resetting = true;
		try {
			const removed = (await client.mail.deleteTemplate(selected)).deleted;
			const reset = selected;
			await load();
			if (templates.some((template) => template.name === reset)) {
				select(reset);
				toast.success(
					removed
						? `Reset ${reset} — ${removed} stored part${removed === 1 ? '' : 's'} removed.`
						: `${reset} was already at its default.`
				);
			} else {
				// Nothing underneath: the stored bodies were the whole template, so a
				// reset removed it. Move the editor to the first surviving template
				// rather than leaving it on a name that no longer exists.
				toast.success(`Reset ${reset} — no file or built-in underneath, so the template is gone.`);
				const first = templates[0];
				if (first) {
					select(first.name);
				} else {
					selected = '';
					creating = false;
					rendered = null;
					renderError = '';
					missing = [];
					draft = { subject: '', html: '', text: '' };
				}
			}
		} catch (err) {
			toast.error(message(err, 'Could not reset the template'));
		} finally {
			resetting = false;
		}
	}

	// ── Preview ──
	// Rendered by the server through the same path a send uses, so what the editor
	// shows cannot differ from what goes out. Debounced, and each response is
	// dropped if a newer draft has superseded it.
	let previewTimer: ReturnType<typeof setTimeout> | undefined;
	let previewSeq = 0;

	async function runPreview() {
		if (!selected) return;
		if (parsedSample.error) {
			rendered = null;
			missing = [];
			renderError = parsedSample.error;
			return;
		}
		// A new name has no stored parts to fall back on, so an incomplete draft
		// has nothing to render yet — the server says as much, but saying it here
		// avoids a request per keystroke.
		if (creating && !complete) {
			rendered = null;
			missing = [];
			renderError = '';
			return;
		}
		const seq = ++previewSeq;
		previewing = true;
		try {
			const previewed = await client.mail.preview(
				selected,
				{ assigns: parsedSample.value, subject: draft.subject, html: draft.html, text: draft.text },
				// Auto-cancellation would abort this render on the next keystroke and
				// reject a promise nobody waits on; the sequence guard does the same
				// job without the noise.
				{ requestKey: null }
			);
			if (seq !== previewSeq) return;
			rendered = previewed;
			// The installed SDK's `MailPreviewResult` predates this field, hence the cast;
			// it is the render's own report of what it left empty.
			missing = (previewed as { missing?: string[] }).missing ?? [];
			renderError = '';
		} catch (err) {
			if (seq !== previewSeq) return;
			rendered = null;
			missing = [];
			// An undefined variable is the most common answer here: the template
			// names something the sample does not supply.
			renderError = message(err, 'Could not render the preview');
		} finally {
			if (seq === previewSeq) previewing = false;
		}
	}

	$effect(() => {
		// Re-render whenever the draft, the sample or the selection changes. The
		// derived key is only read to register those dependencies.
		if (draftState) schedulePreview();
	});

	const draftState = $derived(JSON.stringify({ selected, creating, sample, draft }));

	function schedulePreview() {
		clearTimeout(previewTimer);
		previewTimer = setTimeout(() => void runPreview(), 350);
	}
</script>

{#snippet layerBadge(layer: Layer | undefined)}
	{#if layer}
		<span
			class="badge badge-sm {layer === 'database'
				? 'badge-primary'
				: layer === 'file'
					? 'badge-ghost'
					: 'badge-neutral'}"
			title={LAYER_HELP[layer]}
		>
			{layer}
		</span>
	{/if}
{/snippet}

{#snippet variableMenu(insert: (name: string) => void)}
	<!--
		`{x}` inserts at the cursor, so a variable can be dropped into a body that is
		already half written rather than retyped at the right place by hand.
	-->
	<details class="dropdown dropdown-end">
		<summary class="btn btn-ghost btn-xs" title="Insert a variable">{'{x}'}</summary>
		<ul
			class="dropdown-content menu z-20 w-44 rounded-box border border-base-300 bg-base-100 p-1 shadow"
		>
			{#each variables as name (name)}
				<li>
					<button type="button" class="font-mono text-xs" onclick={() => insert(name)}>
						{`{{ ${name} }}`}
					</button>
				</li>
			{/each}
		</ul>
	</details>
{/snippet}

<h2 class="mb-4 text-lg font-semibold">Mail Templates</h2>
<div class="mb-4 text-sm text-base-content/60">
	<p>
		Every email the server can send. A template resolves three layers — <strong>database</strong>
		(saved here), then <strong>file</strong> (your template directory), then
		<strong>builtin</strong> — and each part reports the one it came from, so an edit that is in effect
		is distinguishable from one that is not.
	</p>
	<p class="mt-1">
		Saving replaces all three parts; resetting deletes the stored ones and falls back to the file or
		the built-in.
	</p>
</div>

<div class="flex flex-col gap-4 xl:flex-row xl:items-start">
	<!-- Template list -->
	<aside class="w-full rounded-box border border-base-300 bg-base-100 p-2 xl:w-64 xl:shrink-0">
		<div class="flex items-center justify-between px-2 py-1">
			<span class="text-[10px] font-semibold tracking-wider text-base-content/40 uppercase">
				Templates
			</span>
			<Button
				class="btn-ghost btn-xs"
				ariaLabel="New template"
				title="New template"
				onclick={startNew}
			>
				<Plus size={14} />
			</Button>
		</div>
		{#if loading}
			<p class="px-3 py-2 text-sm text-base-content/50">Loading…</p>
		{:else if templates.length === 0}
			<p class="px-3 py-2 text-sm text-base-content/50">No templates.</p>
		{:else}
			<ul class="flex max-h-[26rem] flex-col gap-0.5 overflow-y-auto">
				{#each templates as template (template.name)}
					<li>
						<button
							class="flex w-full cursor-pointer items-center justify-between gap-2 rounded-field border-none px-3 py-1.5 text-left text-sm text-base-content transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
							class:bg-base-200={template.name === selected}
							class:font-medium={template.name === selected}
							onclick={() => void choose(template.name)}
						>
							<span class="truncate font-mono text-xs">{template.name}</span>
							{@render layerBadge(template.source)}
						</button>
					</li>
				{/each}
			</ul>
		{/if}
		{#if creating}
			<div
				class="mt-1 flex items-center justify-between gap-2 rounded-field bg-base-200 px-3 py-1.5 text-sm"
			>
				<span class="truncate font-mono text-xs">{selected}</span>
				<span class="badge badge-warning badge-sm" title="Not stored yet">new</span>
			</div>
		{/if}
	</aside>

	<!-- Editor + preview -->
	<div class="grid min-w-0 flex-1 gap-4 lg:grid-cols-2">
		<form
			class="rounded-box border border-base-300 bg-base-100 p-6"
			onsubmit={(e) => {
				e.preventDefault();
				void save();
			}}
		>
			<div class="mb-4 flex flex-wrap items-center justify-between gap-2">
				<div class="min-w-0">
					<h3 class="truncate font-mono text-sm font-semibold">{selected || '—'}</h3>
					<p class="text-xs text-base-content/50">{sourceNotice}</p>
				</div>
				<div class="flex items-center gap-2">
					{#if resettable}
						<Button class="btn-outline btn-sm" loading={resetting} onclick={reset}>
							<Undo2 size={14} />
							Reset
						</Button>
					{/if}
					<Button
						class="btn-primary btn-sm"
						type="submit"
						loading={saving}
						disabled={saving || !complete || !dirty}
					>
						{creating ? 'Create template' : 'Save changes'}
					</Button>
				</div>
			</div>

			<div class="grid gap-5">
				<!-- Subject -->
				<div>
					<div class="mb-1.5 flex items-center justify-between gap-2">
						<span class="text-xs font-medium text-base-content/60">Subject</span>
						{#if !creating}{@render layerBadge(current?.parts.subject)}{/if}
					</div>
					<input
						class="input w-full"
						placeholder={PLACEHOLDER_SUBJECT}
						bind:value={draft.subject}
					/>
				</div>

				<!-- HTML body: the document, or the markup it is built from. -->
				<div>
					<div class="mb-1.5 flex items-center justify-between gap-2">
						<span class="text-xs font-medium text-base-content/60">HTML body</span>
						<div class="flex items-center gap-1.5">
							{@render variableMenu(insertVariable)}
							<div class="join">
								<Button
									class="join-item btn-xs {htmlMode === 'rich' ? 'btn-active' : ''}"
									onclick={() => (htmlMode = 'rich')}
								>
									Rich
								</Button>
								<Button
									class="join-item btn-xs {htmlMode === 'html' ? 'btn-active' : ''}"
									onclick={() => (htmlMode = 'html')}
								>
									HTML
								</Button>
							</div>
							{#if !creating}{@render layerBadge(current?.parts.html)}{/if}
						</div>
					</div>
					<!--
						One value, two views: the rich editor writes the same markup the source
						shows, so switching never loses an edit. Markup that the rich editor
						cannot represent (an inline style, say) is only preserved in `HTML`.
					-->
					{#if htmlMode === 'rich'}
						<div class="h-96 overflow-auto rounded-field border border-base-300 bg-base-100">
							<RichEditor bind:value={draft.html} bind:this={richBody} format="html" />
						</div>
					{:else}
						<textarea
							id="mail-template-html"
							class="input h-96 w-full resize-y font-mono text-xs outline-none focus:outline-none"
							bind:this={htmlArea}
							placeholder={PLACEHOLDER_HTML}
							bind:value={draft.html}></textarea>
					{/if}
					<p class="mt-1 text-xs text-base-content/40">
						{htmlMode === 'rich'
							? 'Editing the document. Switch to HTML for the markup exactly as it will be sent.'
							: 'The markup as it will be sent, variables included.'}
					</p>
				</div>

				<!-- Plain-text body -->
				<div>
					<div class="mb-1.5 flex items-center justify-between gap-2">
						<span class="text-xs font-medium text-base-content/60">Plain-text body</span>
						{#if !creating}{@render layerBadge(current?.parts.text)}{/if}
					</div>
					<textarea
						id="mail-template-text"
						class="input w-full font-mono text-xs outline-none focus:outline-none"
						rows="6"
						placeholder={PLACEHOLDER_TEXT}
						bind:value={draft.text}></textarea>
					<p class="mt-1 text-xs text-base-content/40">
						Sent as the alternative for mail clients that show no HTML.
					</p>
				</div>

				<!-- Sample values -->
				<div>
					<span class="mb-1.5 block text-xs font-medium text-base-content/60">
						Sample values (JSON) — used by the preview and the test send
					</span>
					<textarea
						id="mail-template-sample"
						class="input w-full font-mono text-xs outline-none focus:outline-none"
						rows="3"
						placeholder={'{ "token": "abc123" }'}
						bind:value={sample}></textarea>
					{#if parsedSample.error}
						<p class="mt-1 text-xs text-error">{parsedSample.error}</p>
					{/if}
					<p class="mt-1 text-xs text-base-content/40">
						<code>name</code> and <code>app_name</code> are filled in by the server.
					</p>
				</div>
			</div>
		</form>

		<div class="flex flex-col gap-4">
			<div class="rounded-box border border-base-300 bg-base-100 p-6">
				<div class="mb-2 flex items-center justify-between gap-2">
					<span class="text-xs text-base-content/50">
						Preview{previewing ? ' — rendering...' : ''}
					</span>
				</div>

				{#if renderError}
					<p class="rounded-field bg-error/10 p-3 font-mono text-xs break-words text-error">
						{renderError}
					</p>
				{:else if rendered}
					<!--
						A message as an inbox shows it: the three headers a recipient reads
						before the body, then what the render left empty, then the body. The
						sandboxed body stays exactly as it was — this is a frame around it.
					-->
					<div class="rounded-field border border-base-300">
						<div class="grid grid-cols-[auto_1fr] items-baseline gap-x-3 gap-y-1 p-3">
							<span class="text-xs text-base-content/50">To</span>
							<span class="truncate font-mono text-xs">{toAddress || '—'}</span>
							<span class="text-xs text-base-content/50">Subject</span>
							<span class="truncate text-sm font-medium">{rendered.subject || '—'}</span>
							<span class="text-xs text-base-content/50">From</span>
							<span class="truncate font-mono text-xs">{sender || '—'}</span>
						</div>
						{#if missing.length}
							<p class="border-t border-base-300 bg-warning/10 px-3 py-1.5 text-xs text-warning">
								Rendered with an empty <code class="font-mono">{missing.join(', ')}</code> — the
								sample values do not supply {missing.length === 1 ? 'it' : 'them'}.
							</p>
						{/if}
						<div class="border-t border-base-300 p-3">
							<!-- The one control inside the frame: which of the two bodies to show. -->
							<Tabs bind:active={previewTab} items={['HTML', 'Plain text']} />
							{#if previewTab === 'HTML'}
								<!--
									Sandboxed with no permissions at all: an email body is untrusted
									input, so it renders in an opaque origin with no scripts, no forms
									and no access to this page — never `{@html}` here.
								-->
								<iframe
									title="Rendered email"
									sandbox=""
									referrerpolicy="no-referrer"
									class="h-96 w-full rounded-field border border-base-300 bg-white"
									srcdoc={rendered.html}
								></iframe>
							{:else}
								<pre
									class="h-96 w-full overflow-auto rounded-field bg-base-200 p-3 font-mono text-xs whitespace-pre-wrap">{rendered.text}</pre>
							{/if}
						</div>
					</div>
				{:else}
					<p class="text-sm text-base-content/50">
						{creating && !complete
							? 'Fill in all three parts to preview the new template.'
							: 'Nothing to render yet.'}
					</p>
				{/if}
			</div>

			{#if selected && !creating}
				<MailTestSend
					template={selected}
					assigns={parsedSample.value}
					description="Queues this template to the address below, rendered with the sample values on this page."
				/>
			{/if}
		</div>
	</div>
</div>
