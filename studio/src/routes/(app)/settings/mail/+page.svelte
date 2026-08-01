<script lang="ts">
	import { client } from '$lib/client';
	import { onMount } from 'svelte';
	import { slide } from 'svelte/transition';
	import Button from '$lib/components/Button.svelte';
	import Input from '$lib/components/Input.svelte';
	import '../settings.css';

	let mailEnabled = $state(false);
	let senderName = $state('');
	let senderAddress = $state('');
	let smtpHost = $state('');
	let smtpPort = $state('587');
	let smtpUser = $state('');
	let smtpPass = $state('');
	let smtpTls = $state(false);
	let smtpAuthMethod = $state('PLAIN');
	let smtpLocalName = $state('');
	let showMoreMail = $state(false);
	let mailSaving = $state(false);

	onMount(async () => {
		try {
			const res = (await client.http.get('/settings')) as Record<string, unknown> | null;
			if (!res) return;
			const mail = (res.mail as Record<string, unknown>) ?? {};
			mailEnabled = (mail.enabled as boolean) ?? false;
			senderName = (mail.sender_name as string) ?? '';
			senderAddress = (mail.sender_address as string) ?? '';
			smtpHost = (mail.host as string) ?? '';
			smtpPort = (mail.port as string) ?? '587';
			smtpUser = (mail.user as string) ?? '';
			smtpPass = (mail.pass as string) ?? '';
			smtpTls = (mail.tls as boolean) ?? false;
			smtpAuthMethod = (mail.auth_method as string) ?? 'PLAIN';
			smtpLocalName = (mail.local_name as string) ?? '';
		} catch {
			// not configured
		}
	});

	async function saveMail() {
		mailSaving = true;
		try {
			await client.http.patch('/settings', {
				mail: {
					enabled: mailEnabled,
					sender_name: senderName || null,
					sender_address: senderAddress || null,
					host: smtpHost || null,
					port: smtpPort || null,
					user: smtpUser || null,
					pass: smtpPass || null,
					tls: smtpTls,
					auth_method: smtpAuthMethod,
					local_name: smtpLocalName || null
				}
			});
		} catch {
			// ignore
		} finally {
			mailSaving = false;
		}
	}
</script>

<h2 class="mb-4 text-lg font-semibold">Mail Settings</h2>
<div class="rounded-box border border-base-300 bg-base-100 p-6">
	<div class="mb-4 text-sm text-base-content/60">
		<p>Configure common settings for sending emails.</p>
	</div>

	<div class="mb-6 flex flex-col gap-3 sm:flex-row sm:items-start">
		<div class="flex-1">
			<Input label="Sender name" placeholder="John Doe" bind:value={senderName} />
		</div>
		<div class="flex-1">
			<Input
				label="Sender address"
				placeholder="noreply@example.com"
				type="email"
				bind:value={senderAddress}
			/>
		</div>
	</div>

	<!-- SMTP toggle -->
	<div class="switch-field mb-4">
		<label class="switch-label" for="mail-enabled">
			<span class="txt">Use SMTP mail server <strong>(recommended)</strong></span>
		</label>
		<label class="switch">
			<input id="mail-enabled" type="checkbox" bind:checked={mailEnabled} />
			<span class="switch-slider"></span>
		</label>
	</div>

	{#if mailEnabled}
		<div transition:slide={{ duration: 150 }}>
			<div class="flex flex-col gap-3 sm:flex-row">
				<div class="flex-[5]">
					<Input
						label="SMTP server host"
						placeholder="smtp.example.com"
						bind:value={smtpHost}
						required
					/>
				</div>
				<div class="flex-[3]">
					<Input label="Port" placeholder="587" bind:value={smtpPort} required />
				</div>
				<div class="flex-[4]">
					<Input label="Username" bind:value={smtpUser} />
				</div>
				<div class="flex-[4]">
					<Input label="Password" type="password" bind:value={smtpPass} />
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
								<label class="field-label">TLS encryption</label>
								<select class="field-input" bind:value={smtpTls}>
									<option value={false}>Auto (StartTLS)</option>
									<option value={true}>Always</option>
								</select>
							</div>
						</div>
						<div class="lg:col-span-4">
							<div class="field">
								<label class="field-label">AUTH method</label>
								<select class="field-input" bind:value={smtpAuthMethod}>
									<option value="PLAIN">PLAIN (default)</option>
									<option value="LOGIN">LOGIN</option>
								</select>
							</div>
						</div>
						<div class="lg:col-span-4">
							<Input
								label="EHLO/HELO domain"
								placeholder="Default to localhost"
								bind:value={smtpLocalName}
							/>
						</div>
					</div>
				</div>
			{/if}
		</div>
	{/if}

	<div class="mt-6 flex items-center justify-end gap-3">
		<Button class="btn-primary" loading={mailSaving} onclick={saveMail}>Save changes</Button>
	</div>
</div>
