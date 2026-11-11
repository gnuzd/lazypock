<script lang="ts">
	import { client } from '$lib/client';
	import { confirmDialog, promptDialog } from '$lib/dialog.svelte';
	import type { MailTemplate, MailTemplateLayer } from 'lazypock';
	import { onMount, tick } from 'svelte';
	import { slide } from 'svelte/transition';
	import { z } from 'zod';
	import {
		Check,
		Cog,
		Layers,
		MailOpen,
		MailPlus,
		Plus,
		Save,
		Search,
		SlidersHorizontal,
		Undo2
	} from '@lucide/svelte';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import MailTestSend from '$lib/components/MailTestSend.svelte';
	import RichEditor from '$lib/components/RichEditor.svelte';
	import Select from '$lib/components/Select.svelte';
	import Switch from '$lib/components/Switch.svelte';
	import { toast } from 'svelte-sonner';
	import { createForm } from '$lib/createForm.svelte';
	import '../settings.css';

	// One page for everything about mail: the SMTP settings, and the templates the
	// server sends. The three tabs are views of the same two sections — the
	// all-in-one one shows both, the other two one each — so nothing is loaded,
	// saved or validated differently from one tab to the next.
	//
	// Four deliberate choices (the template editor's own, kept here):
	//
	// * A template is saved as a whole (`PUT`), because a template only exists
	//   once it is complete — a part-at-a-time write could leave a subject saved
	//   against a body that does not parse.
	// * The plain-text alternative is not edited here: the server derives it from the
	//   HTML every time the template is saved, so the two bodies cannot drift apart.
	// * The preview is a real render by the server, in a **sandboxed** iframe
	//   (`sandbox=""`, so no scripts and no access to the Studio's origin) — an
	//   email body is untrusted input, never `{@html}` in this document.
	// * Reset deletes the stored bodies, which is destructive enough to ask first;
	//   it falls back to a file in the template directory, then to the built-in.

	type Tab = 'all' | 'settings' | 'templates';
	const TABS: { id: Tab; label: string; icon: typeof Cog }[] = [
		{ id: 'all', label: 'All-in-One View', icon: Layers },
		{ id: 'settings', label: 'Mail Settings', icon: Cog },
		{ id: 'templates', label: 'Mail Templates', icon: MailPlus }
	];
	let tab = $state<Tab>('all');

	// ── Server & sender settings ──

	const mailSchema = z
		.object({
			mailEnabled: z.boolean(),
			senderName: z.string(),
			senderAddress: z.string().email('Invalid email').optional().or(z.literal('')),
			smtpHost: z.string(),
			smtpPort: z.string(),
			smtpUser: z.string(),
			smtpPass: z.string(),
			smtpTls: z.boolean(),
			smtpAuthMethod: z.string(),
			smtpLocalName: z.string()
		})
		.superRefine((data, ctx) => {
			if (data.mailEnabled && !data.smtpHost.trim()) {
				ctx.addIssue({ code: 'custom', path: ['smtpHost'], message: 'SMTP host is required' });
			}
			if (data.mailEnabled && !data.smtpPort.trim()) {
				ctx.addIssue({ code: 'custom', path: ['smtpPort'], message: 'Port is required' });
			}
		});

	let mailForm = createForm(mailSchema, {
		mailEnabled: false,
		senderName: '',
		senderAddress: '',
		smtpHost: '',
		smtpPort: '587',
		smtpUser: '',
		smtpPass: '',
		smtpTls: false,
		smtpAuthMethod: 'PLAIN',
		smtpLocalName: ''
	});
	let showMoreMail = $state(false);
	/** The last loaded or saved settings as JSON, so the header can tell saved from unsaved. */
	let mailBaseline = $state('');

	const mailDirty = $derived(JSON.stringify(mailForm.form) !== mailBaseline);

	function snapshotMail() {
		mailBaseline = JSON.stringify(mailForm.form);
	}

	/** Who the preview's `From:` line shows — the sender as configured here. */
	const sender = $derived(
		mailForm.form.senderAddress
			? mailForm.form.senderName
				? `${mailForm.form.senderName} <${mailForm.form.senderAddress}>`
				: mailForm.form.senderAddress
			: ''
	);

	async function saveMail(data: z.infer<typeof mailSchema>) {
		try {
			await client.http.patch('/settings', {
				mail: {
					enabled: data.mailEnabled,
					sender_name: data.senderName || null,
					sender_address: data.senderAddress || null,
					host: data.smtpHost || null,
					port: data.smtpPort || null,
					user: data.smtpUser || null,
					pass: data.smtpPass || null,
					tls: data.smtpTls,
					auth_method: data.smtpAuthMethod,
					local_name: data.smtpLocalName || null
				}
			});
			snapshotMail();
		} catch (err) {
			toast.error(message(err, 'Could not save the mail settings'));
		}
	}

	// ── Templates ──

	type Layer = MailTemplateLayer;

	/** The parts the editor writes; the plain-text alternative follows from `html`. */
	const PARTS = ['subject', 'html'] as const;
	type Part = (typeof PARTS)[number];

	/** Every part a stored template can carry — reported when one is reset. */
	const STORED_PARTS = ['subject', 'html', 'text'] as const;

	const LAYER_HELP: Record<Layer, string> = {
		database: 'Saved here, and the one in effect.',
		file: 'From a file in the deployment’s template directory.',
		builtin: 'Shipped with the server.'
	};

	// Held in constants because Svelte reads a literal `{{ ... }}` in an attribute as
	// an object expression rather than as text.
	const PLACEHOLDER_SUBJECT = 'Your receipt from {{ app_name }}';
	const PLACEHOLDER_HTML = '<p>Hello {{ name }},</p>';

	let templates = $state<MailTemplate[]>([]);
	let loading = $state(true);
	/** The name being edited — a stored one, or a new one that is not saved yet. */
	let selected = $state('');
	/** True while the selection is a name nothing has stored yet. */
	let creating = $state(false);
	let draft = $state<Record<Part, string>>({ subject: '', html: '' });
	/** The sample values handed to `{{ ... }}`, as JSON. */
	let sample = $state('{}');
	let saving = $state(false);
	let resetting = $state(false);
	let rendered = $state<{ subject: string; html: string; text: string } | null>(null);
	let renderError = $state('');
	let previewing = $state(false);
	/** How the HTML body is being edited: the rendered document, or its source. */
	let htmlMode = $state<'rich' | 'html'>('rich');
	/** The window the preview is framed at. */
	let device = $state<'desktop' | 'mobile'>('desktop');
	/** Filters the template list by name and subject. */
	let query = $state('');
	/** The rich editor, so `{x}` can insert a variable at its cursor. */
	let richBody = $state<{ insertText: (text: string) => void } | null>(null);
	/** The source textarea, for the same reason while the raw body is being edited. */
	let htmlArea = $state<HTMLTextAreaElement | null>(null);
	/** The variables the last preview left empty, because the sample does not supply them. */
	let missing = $state<string[]>([]);
	/** The address a test send starts from. */
	let meEmail = $state('');
	/** True while the header button is saving both sections. */
	let savingPage = $state(false);

	/** The stored template behind the editor, when there is one. */
	const current = $derived(templates.find((t) => t.name === selected) ?? null);

	const shown = $derived.by(() => {
		const needle = query.trim().toLowerCase();
		if (!needle) return templates;
		return templates.filter(
			(t) =>
				t.name.toLowerCase().includes(needle) || (t.subject ?? '').toLowerCase().includes(needle)
		);
	});

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
			? 'A new template — a subject and an HTML body are needed before it can be saved.'
			: `Comes from ${current?.source ?? '—'}.`
	);
	const pageDirty = $derived(mailDirty || dirty);

	/** The `{{ ... }}` names the draft uses, for the variable menu. */
	const variables = $derived.by(() => {
		const names: string[] = [];
		for (const part of PARTS) {
			for (const [, name] of draft[part].matchAll(/\{\{\s*([A-Za-z_][A-Za-z0-9_.]*)/g)) {
				if (!names.includes(name)) names.push(name);
			}
		}
		// `name` is filled in by the server whatever the sample says, so the menu always
		// has something to offer a body that uses no variables yet.
		return (names.length ? names : ['name']).sort();
	});

	/** Insert a variable into the HTML body, wherever it is being edited. */
	function insertVariable(name: string) {
		const token = `{{ ${name} }}`;
		if (htmlMode === 'rich') {
			richBody?.insertText(token);
			return;
		}
		insertIntoArea(htmlArea, token);
	}

	/** Insert `text` at the cursor of a textarea and put the caret after it. */
	function insertIntoArea(area: HTMLTextAreaElement | null, text: string) {
		if (!area) return;
		const { selectionStart: start, selectionEnd: end } = area;
		draft.html = draft.html.slice(0, start) + text + draft.html.slice(end);
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

	/** Size the preview frame to the document it rendered, so a long body is not cut off. */
	function fitFrame(frame: HTMLIFrameElement) {
		const doc = frame.contentDocument;
		if (doc) frame.style.height = `${doc.documentElement.scrollHeight}px`;
	}

	async function loadTemplates() {
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
		// `requestKey: null` on both: the SDK auto-cancels a request with the same
		// method and path, and the layout (plus MailTestSend) asks for `/me` and
		// `/settings` on the very same page load — one of them would abort with no
		// visible cause.
		try {
			const me = (await client.me({ requestKey: null })) as Record<string, unknown> | null;
			meEmail = (me?.email as string) ?? '';
		} catch {
			// the frame shows a dash
		}
		try {
			const res = (await client.http.get('/settings', {
				requestKey: null
			})) as Record<string, unknown> | null;
			const mail = (res?.mail as Record<string, unknown>) ?? {};
			const v = mailForm.form;
			v.mailEnabled = (mail.enabled as boolean) ?? false;
			v.senderName = (mail.sender_name as string) ?? '';
			v.senderAddress = (mail.sender_address as string) ?? '';
			v.smtpHost = (mail.host as string) ?? '';
			v.smtpPort = (mail.port as string) ?? '587';
			v.smtpUser = (mail.user as string) ?? '';
			v.smtpPass = (mail.pass as string) ?? '';
			v.smtpTls = (mail.tls as boolean) ?? false;
			v.smtpAuthMethod = (mail.auth_method as string) ?? 'PLAIN';
			v.smtpLocalName = (mail.local_name as string) ?? '';
		} catch {
			// not configured
		}
		snapshotMail();

		await loadTemplates();
		const first = templates[0];
		if (first && !selected) select(first.name);
	});

	function select(name: string) {
		const template = templates.find((t) => t.name === name);
		if (!template) return;
		selected = name;
		creating = false;
		rendered = null;
		renderError = '';
		missing = [];
		draft = { subject: template.subject, html: template.html };
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
				'A name for the template — no spaces. It is stored once a subject and an HTML body are filled in.',
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
		draft = { subject: '', html: '' };
	}

	/** True when the draft was stored. */
	async function save() {
		if (!selected || saving || !complete) return false;
		saving = true;
		try {
			// An empty `text`: the server derives the plain-text alternative from the HTML,
			// which is the only way the two bodies stay in step with each other.
			await client.mail.saveTemplate(selected, {
				subject: draft.subject,
				html: draft.html,
				text: ''
			});
			toast.success(`Saved the ${selected} template.`);
			creating = false;
			// Reload rather than patching the list from the response: a new name is not in
			// `templates` yet, and the reload is also what reports the layer each part now
			// resolves from. `select()` is deliberately not called — it would blank the
			// preview, which is already the render of what was just stored.
			await loadTemplates();
			return true;
		} catch (err) {
			// A body that does not parse is refused at save time rather than
			// queueing an email that would fail where nobody is watching.
			toast.error(message(err, 'Could not save the template'));
			return false;
		} finally {
			saving = false;
		}
	}

	/** The header button: both sections, because the page is one form to its reader. */
	async function savePage() {
		if (savingPage) return;
		savingPage = true;
		try {
			if (mailForm.validateAll()) await saveMail(mailForm.form);
			if (!dirty) return;
			if (!complete) {
				toast.error('Fill in the subject and the HTML body before saving the template.');
				return;
			}
			await save();
		} finally {
			savingPage = false;
		}
	}

	async function reset() {
		if (!selected || resetting || !resettable) return;
		const stored = STORED_PARTS.filter((part) => current?.parts[part] === 'database');
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
			await loadTemplates();
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
					draft = { subject: '', html: '' };
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
				// No `text` draft: the editor no longer edits that part, and the frame below
				// only shows the HTML one.
				{ assigns: parsedSample.value, subject: draft.subject, html: draft.html },
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
		<summary class="cursor-pointer text-[11px]" title="Insert a variable">{'{x}'}</summary>
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

{#snippet serverSection()}
	<section>
		<h2 class="mb-3 flex items-center gap-2 text-lg font-semibold">
			<SlidersHorizontal size={18} class="opacity-60" />
			Mail Server &amp; Sender Configuration
		</h2>
		<!--
			The form is `contents` so its two cards are grid cells while the test-send
			card — which has a recipient picker, and Enter-driven fields of its own —
			stays outside the form it does not submit.
		-->
		<div class="grid gap-4 xl:grid-cols-3">
			<form class="contents" autocomplete="off" use:mailForm.enhance={saveMail}>
				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<h3 class="font-semibold">Sender Information</h3>
					<p class="mt-0.5 mb-4 text-xs text-base-content/50">
						Default email address and label seen by recipients.
					</p>
					<div class="grid gap-3">
						<Input
							label="Sender Name"
							placeholder="John Doe"
							bind:value={mailForm.form.senderName}
						/>
						<Input
							label="Sender Address"
							placeholder="no-reply@example.com"
							type="email"
							bind:value={mailForm.form.senderAddress}
							error={mailForm.errors.senderAddress}
						/>
					</div>
				</div>

				<div class="rounded-box border border-base-300 bg-base-100 p-6">
					<h3 class="font-semibold">Server Settings</h3>
					<p class="mt-0.5 mb-4 text-xs text-base-content/50">
						Configure external SMTP mail gateway.
					</p>
					<Switch bind:checked={mailForm.form.mailEnabled}>
						Use SMTP mail server <strong>(recommended)</strong>
					</Switch>

					{#if mailForm.form.mailEnabled}
						<div transition:slide={{ duration: 150 }}>
							<div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
								<Input
									label="SMTP Host"
									placeholder="smtp.mailgun.org"
									bind:value={mailForm.form.smtpHost}
									error={mailForm.errors.smtpHost}
									required
								/>
								<Input
									label="Port"
									placeholder="587"
									bind:value={mailForm.form.smtpPort}
									error={mailForm.errors.smtpPort}
									required
								/>
								<Input
									label="Username"
									placeholder="postmaster@mg.example.com"
									bind:value={mailForm.form.smtpUser}
								/>
								<Input
									label="Password"
									type="password"
									autocomplete="new-password"
									bind:value={mailForm.form.smtpPass}
								/>
							</div>

							<button
								type="button"
								class="mt-3 cursor-pointer border-none bg-transparent text-sm text-base-content/50 hover:text-base-content"
								onclick={() => (showMoreMail = !showMoreMail)}
							>
								{showMoreMail ? 'Hide more options' : 'Show more options'}
							</button>

							{#if showMoreMail}
								<div
									transition:slide={{ duration: 150 }}
									class="grid grid-cols-1 gap-3 sm:grid-cols-2"
								>
									<div class="field">
										<span class="field-label">TLS encryption</span>
										<Select
											variant="plain"
											triggerClass="w-full py-2.5"
											options={[
												{ value: false, label: 'Auto (StartTLS)' },
												{ value: true, label: 'Always' }
											]}
											bind:value={mailForm.form.smtpTls}
										/>
									</div>
									<div class="field">
										<span class="field-label">AUTH method</span>
										<Select
											variant="plain"
											triggerClass="w-full py-2.5"
											options={[
												{ value: 'PLAIN', label: 'PLAIN (default)' },
												{ value: 'LOGIN', label: 'LOGIN' }
											]}
											bind:value={mailForm.form.smtpAuthMethod}
										/>
									</div>
									<Input
										label="EHLO/HELO domain"
										placeholder="Default to localhost"
										bind:value={mailForm.form.smtpLocalName}
									/>
								</div>
							{/if}
						</div>
					{/if}
				</div>
			</form>

			<MailTestSend
				id="test-recipient-server"
				title="Send a Test Email"
				description="Use saved settings to verify delivery."
			/>
		</div>
	</section>
{/snippet}

{#snippet templatesSection()}
	<section>
		<h2 class="mb-3 flex items-center gap-2 text-lg font-semibold">
			<MailOpen size={18} class="opacity-60" />
			Mail Templates &amp; Live Editor
		</h2>

		<div class="grid gap-4 xl:grid-cols-[15rem_minmax(0,1fr)_24rem] xl:items-start">
			<!-- Template list -->
			<aside class="w-full rounded-box border border-base-300 bg-base-100 p-3">
				<div class="flex items-center justify-between pl-1">
					<span class="text-[10px] font-semibold tracking-wider text-base-content/40 uppercase">
						Templates
					</span>
					<Button
						class="btn-ghost btn-xs gap-1"
						ariaLabel="New template"
						title="New template"
						onclick={startNew}
					>
						<Plus size={14} />
						New
					</Button>
				</div>
				<div class="relative mt-2">
					<Search size={13} class="absolute top-2.5 left-2.5 opacity-40" />
					<input
						class="input input-sm w-full pl-7"
						placeholder="Search templates..."
						aria-label="Search templates"
						bind:value={query}
					/>
				</div>

				{#if loading}
					<p class="px-1 py-2 text-sm text-base-content/50">Loading...</p>
				{:else if shown.length === 0}
					<p class="px-1 py-2 text-sm text-base-content/50">
						{templates.length === 0 ? 'No templates.' : 'No template matches that.'}
					</p>
				{:else}
					<ul class="mt-2 flex flex-col gap-0.5">
						{#each shown as template (template.name)}
							<li>
								<button
									class="flex w-full cursor-pointer flex-col gap-0.5 rounded-field border-none px-2.5 py-1.5 text-left text-base-content transition-[background] duration-(--animation-speed-fast) hover:bg-base-200"
									class:bg-base-200={template.name === selected}
									class:font-medium={template.name === selected}
									onclick={() => void choose(template.name)}
								>
									<span class="flex w-full items-center justify-between gap-2">
										<span class="truncate font-mono text-xs">{template.name}</span>
										{@render layerBadge(template.source)}
									</span>
									<span class="w-full truncate text-xs text-base-content/50">
										{template.subject}
									</span>
								</button>
							</li>
						{/each}
					</ul>
				{/if}

				{#if creating}
					<div
						class="mt-1 flex items-center justify-between gap-2 rounded-field bg-base-200 px-2.5 py-1.5 text-sm"
					>
						<span class="truncate font-mono text-xs">{selected}</span>
						<span class="badge badge-warning badge-sm" title="Not stored yet">new</span>
					</div>
				{/if}

				<p class="mt-3 border-t border-base-300 pt-2 pl-1 text-xs text-base-content/50">
					Total templates: <strong class="text-base-content">{templates.length}</strong>
				</p>
			</aside>

			<!-- Editor -->
			<form
				class="min-w-0 rounded-box border border-base-300 bg-base-100 p-6"
				onsubmit={(e) => {
					e.preventDefault();
					void save();
				}}
			>
				<div class="mb-4 flex flex-wrap items-center justify-between gap-2">
					<div class="flex min-w-0 items-center gap-2">
						<span class="text-xs text-base-content/50">Editing key:</span>
						<code class="rounded-field border border-base-300 bg-base-200 px-2 py-0.5 text-xs">
							{selected || '—'}
						</code>
						{#if resettable}
							<Button class="btn-ghost btn-xs gap-1" loading={resetting} onclick={reset}>
								<Undo2 size={13} />
								Reset
							</Button>
						{/if}
					</div>
					<div class="join">
						<Button
							class="join-item btn-xs {htmlMode === 'rich' ? 'btn-active' : ''}"
							onclick={() => (htmlMode = 'rich')}
						>
							Rich Text
						</Button>
						<Button
							class="join-item btn-xs {htmlMode === 'html' ? 'btn-active' : ''}"
							onclick={() => (htmlMode = 'html')}
						>
							HTML
						</Button>
					</div>
				</div>
				<p class="mb-4 text-xs text-base-content/50">{sourceNotice}</p>

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

					<!-- HTML body: the rendered document, or the markup it is built from. -->
					<div>
						<div class="mb-1.5 flex items-center justify-between gap-2">
							<span class="text-xs font-medium text-base-content/60">HTML Body</span>
							<div class="flex items-center gap-2">
								{@render variableMenu(insertVariable)}
								{#if !creating}{@render layerBadge(current?.parts.html)}{/if}
							</div>
						</div>
						{#if htmlMode === 'rich'}
							<RichEditor bind:value={draft.html} bind:this={richBody} format="html" />
						{:else}
							<textarea
								id="mail-template-html"
								class="textarea min-h-56 w-full font-mono text-xs"
								placeholder={PLACEHOLDER_HTML}
								bind:this={htmlArea}
								bind:value={draft.html}></textarea>
						{/if}
					</div>
				</div>

				<div class="mt-5 border-t border-base-300 pt-3">
					<span class="text-xs text-base-content/50">Available variables (Click to insert):</span>
					<div class="mt-2 flex flex-wrap gap-1.5">
						{#each variables as name (name)}
							<button
								type="button"
								class="cursor-pointer rounded-field border border-base-300 bg-base-200 px-2 py-0.5 font-mono text-[11px] hover:bg-base-300"
								onclick={() => insertVariable(name)}
							>
								{`{{ ${name} }}`}
							</button>
						{/each}
					</div>
					{#if missing.length}
						<p class="mt-2 text-xs text-warning">
							Not filled in by the sample: {missing.join(', ')}
						</p>
					{/if}
					<details class="mt-3">
						<summary class="cursor-pointer text-xs text-base-content/50">
							Sample data — the values the preview fills {'{{ ... }}'} with
						</summary>
						<textarea
							id="mail-template-sample"
							class="textarea mt-2 min-h-20 w-full font-mono text-xs"
							spellcheck="false"
							bind:value={sample}></textarea>
						{#if parsedSample.error}
							<p class="mt-1 text-xs text-error">{parsedSample.error}</p>
						{/if}
					</details>
				</div>
			</form>

			<!-- Live preview -->
			<div class="flex flex-col gap-4">
				<div class="rounded-box border border-base-300 bg-base-100 p-3">
					<div class="mb-2 flex items-center justify-between gap-2">
						<h3 class="font-semibold">Live Preview</h3>
						<div class="join">
							<Button
								class="join-item btn-xs {device === 'desktop' ? 'btn-active' : ''}"
								onclick={() => (device = 'desktop')}
							>
								Desktop
							</Button>
							<Button
								class="join-item btn-xs {device === 'mobile' ? 'btn-active' : ''}"
								onclick={() => (device = 'mobile')}
							>
								Mobile
							</Button>
						</div>
					</div>

					<div
						class="mx-auto w-full overflow-hidden rounded-field border border-base-300 bg-white"
						class:max-w-90={device === 'mobile'}
					>
						{#if rendered}
							<div class="border-b border-base-300 px-4 pt-3 pb-2 text-xs text-black">
								{#each [{ label: 'From', value: sender }, { label: 'To', value: toAddress }, { label: 'Subject', value: rendered.subject }] as row (row.label)}
									<div class="flex justify-between gap-3">
										<span class="shrink-0 text-base-content/40">{row.label}:</span>
										<span class="truncate text-right">{row.value || '—'}</span>
									</div>
								{/each}
							</div>
							<!--
								`sandbox=""`: an email body is untrusted input, so this frame gets
								no scripts and no access to the Studio's origin — never `{@html}`.
							-->
							<iframe
								title="Rendered email"
								sandbox=""
								srcdoc={rendered.html}
								onload={(e) => fitFrame(e.currentTarget as HTMLIFrameElement)}
								class="w-full border-0 bg-white"
							></iframe>
						{:else if renderError}
							<p class="p-4 text-sm text-error">{renderError}</p>
						{:else if previewing}
							<p class="p-4 text-sm text-base-content/50">Rendering...</p>
						{:else}
							<p class="p-4 text-sm text-base-content/50">Pick a template to preview it.</p>
						{/if}
					</div>
				</div>

				<MailTestSend
					id="test-recipient-template"
					title="Test Template Delivery"
					description="Send rendered output with sample data."
					template={selected}
					assigns={parsedSample.value}
				/>
			</div>
		</div>
	</section>
{/snippet}

<div class="flex flex-col gap-4">
	<header class="flex flex-wrap items-start justify-between gap-3 border-b border-base-300 pb-4">
		<div>
			<h1 class="text-xl font-semibold">Mail Management</h1>
			<p class="mt-1 text-sm text-base-content/50">
				Configure SMTP delivery, sender details, and live email template customization.
			</p>
		</div>
		<div class="flex items-center gap-3">
			{#if pageDirty}
				<span class="text-sm text-base-content/50">Unsaved changes</span>
			{:else}
				<span class="flex items-center gap-1.5 text-sm text-success">
					<Check size={14} />
					All changes saved
				</span>
			{/if}
			<!--
				One button for the whole page: the settings and the open template are one
				form to whoever is reading it. `Button` does not forward `form=`, hence
				the click handler rather than a submit button outside the form.
			-->
			<Button
				class="btn-primary gap-1.5"
				loading={savingPage}
				disabled={!pageDirty}
				onclick={() => void savePage()}
			>
				<Save size={15} />
				Save changes
			</Button>
		</div>
	</header>

	<nav class="flex gap-1 border-b border-base-300" aria-label="Mail views">
		{#each TABS as t (t.id)}
			<button
				type="button"
				class="-mb-px flex cursor-pointer items-center gap-2 border-b-2 border-transparent px-3 py-2 text-sm text-base-content/60 hover:text-base-content"
				class:border-primary={tab === t.id}
				class:font-medium={tab === t.id}
				class:text-base-content={tab === t.id}
				aria-current={tab === t.id ? 'page' : undefined}
				onclick={() => (tab = t.id)}
			>
				<t.icon size={15} class="opacity-60" />
				{t.label}
			</button>
		{/each}
	</nav>

	<div class="grid gap-8">
		{#if tab === 'all' || tab === 'settings'}{@render serverSection()}{/if}
		{#if tab === 'all' || tab === 'templates'}{@render templatesSection()}{/if}
	</div>
</div>
