<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { slide } from 'svelte/transition';
	import { z } from 'zod';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import { createForm } from '$lib/createForm.svelte';
	import '../settings.css';

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

	let mailForm = $state(
		createForm(mailSchema, {
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
		})
	);
	let showMoreMail = $state(false);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			const mail = (res.mail as Record<string, unknown>) ?? {};
			const v = mailForm.values;
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
	});

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
		} catch {
			// ignore
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Mail Settings</h2>
<form
	class="rounded-box border border-base-300 bg-base-100 p-6"
	onsubmit={(e) => mailForm.handleSubmit(e, saveMail)}
>
	<div class="mb-4 text-sm text-base-content/60">
		<p>Configure common settings for sending emails.</p>
	</div>

	<div class="mb-6 flex flex-col gap-3 sm:flex-row sm:items-start">
		<div class="flex-1">
			<Input label="Sender name" placeholder="John Doe" bind:value={mailForm.values.senderName} />
		</div>
		<div class="flex-1">
			<Input
				label="Sender address"
				placeholder="noreply@example.com"
				type="email"
				bind:value={mailForm.values.senderAddress}
				error={mailForm.errors.senderAddress}
			/>
		</div>
	</div>

	<!-- SMTP toggle -->
	<div class="switch-field mb-4">
		<label class="switch-label" for="mail-enabled">
			<span class="txt">Use SMTP mail server <strong>(recommended)</strong></span>
		</label>
		<label class="switch">
			<input id="mail-enabled" type="checkbox" bind:checked={mailForm.values.mailEnabled} />
			<span class="switch-slider"></span>
		</label>
	</div>

	{#if mailForm.values.mailEnabled}
		<div transition:slide={{ duration: 150 }}>
			<div class="flex flex-col gap-3 sm:flex-row">
				<div class="flex-[5]">
					<Input
						label="SMTP server host"
						placeholder="smtp.example.com"
						bind:value={mailForm.values.smtpHost}
						error={mailForm.errors.smtpHost}
						required
					/>
				</div>
				<div class="flex-[3]">
					<Input
						label="Port"
						placeholder="587"
						bind:value={mailForm.values.smtpPort}
						error={mailForm.errors.smtpPort}
						required
					/>
				</div>
				<div class="flex-[4]">
					<Input label="Username" bind:value={mailForm.values.smtpUser} />
				</div>
				<div class="flex-[4]">
					<Input label="Password" type="password" bind:value={mailForm.values.smtpPass} />
				</div>
			</div>

			<button
				type="button"
				class="mt-2 mb-4 cursor-pointer border-none bg-transparent text-sm text-base-content/50 hover:text-base-content"
				onclick={() => (showMoreMail = !showMoreMail)}
			>
				{showMoreMail ? 'Hide more options' : 'Show more options'}
			</button>

			{#if showMoreMail}
				<div transition:slide={{ duration: 150 }}>
					<div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-12">
						<div class="lg:col-span-4">
							<div class="field">
								<label class="field-label" for="smtp-tls">TLS encryption</label>
								<select id="smtp-tls" class="field-input" bind:value={mailForm.values.smtpTls}>
									<option value={false}>Auto (StartTLS)</option>
									<option value={true}>Always</option>
								</select>
							</div>
						</div>
						<div class="lg:col-span-4">
							<div class="field">
								<label class="field-label" for="smtp-auth">AUTH method</label>
								<select
									id="smtp-auth"
									class="field-input"
									bind:value={mailForm.values.smtpAuthMethod}
								>
									<option value="PLAIN">PLAIN (default)</option>
									<option value="LOGIN">LOGIN</option>
								</select>
							</div>
						</div>
						<div class="lg:col-span-4">
							<Input
								label="EHLO/HELO domain"
								placeholder="Default to localhost"
								bind:value={mailForm.values.smtpLocalName}
							/>
						</div>
					</div>
				</div>
			{/if}
		</div>
	{/if}

	<div class="mt-6 flex items-center justify-end gap-3">
		<Button class="btn-primary" loading={mailForm.submitting} type="submit">Save changes</Button>
	</div>
</form>
