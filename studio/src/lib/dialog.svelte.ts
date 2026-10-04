/**
 * App-wide modal dialogs.
 *
 * The Studio never uses `window.confirm` / `window.alert` / `window.prompt`:
 * they are unstyled, block the whole page and cannot be themed. Every
 * confirmation, error notice and text prompt goes through the single
 * `<AppDialog />` mounted in the root layout instead:
 *
 * ```ts
 * if (!(await confirmDialog({ title: 'Delete file', message: `Delete "${name}"?`, variant: 'error' }))) return;
 * await alertDialog({ title: 'Delete failed', message: (e as Error).message, variant: 'error' });
 * const url = await promptDialog({ title: 'Insert link', message: 'Link URL' });
 * ```
 *
 * `confirmDialog` resolves `true`/`false`, `promptDialog` resolves the entered
 * string or `null` when cancelled, and `alertDialog` resolves once dismissed.
 */

export type DialogVariant = 'primary' | 'error';

export interface DialogRequest {
	kind: 'confirm' | 'alert' | 'prompt';
	title?: string;
	message: string;
	/** Extra lines shown under the message (e.g. the records using a file). */
	details?: string[];
	confirmLabel?: string;
	cancelLabel?: string;
	variant?: DialogVariant;
	placeholder?: string;
	/** Initial value for `kind: 'prompt'`. */
	value?: string;
}

interface PendingDialog {
	request: DialogRequest;
	resolve: (value: string | boolean | null) => void;
}

let pending = $state<PendingDialog | null>(null);

/** The dialog currently shown — read by `<AppDialog />`. */
export function currentDialog(): PendingDialog | null {
	return pending;
}

/** Settle the open dialog. Called by `<AppDialog />`. */
export function settleDialog(value: string | boolean | null): void {
	const current = pending;
	pending = null;
	current?.resolve(value);
}

function open(request: DialogRequest): Promise<string | boolean | null> {
	return new Promise((resolve) => {
		pending = { request, resolve };
	});
}

/** Ask the user to confirm. Resolves `false` when cancelled or dismissed. */
export async function confirmDialog(options: Omit<DialogRequest, 'kind'>): Promise<boolean> {
	return (await open({ kind: 'confirm', ...options })) === true;
}

/** Informational dialog with a single button (replaces `alert()`). */
export async function alertDialog(options: Omit<DialogRequest, 'kind'>): Promise<void> {
	await open({ kind: 'alert', ...options });
}

/** Ask for a single line of text (replaces `prompt()`). `null` when cancelled. */
export async function promptDialog(options: Omit<DialogRequest, 'kind'>): Promise<string | null> {
	const value = await open({ kind: 'prompt', ...options });
	return typeof value === 'string' ? value : null;
}
