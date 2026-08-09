import type { z } from 'zod';

/**
 * Superform-style form API, reactive and accessible anywhere the form object is in scope.
 *
 * ```ts
 * const form = createForm(schema, { email: '', password: '' });
 *
 * // in markup
 * <Input bind:value={form.values.email} error={form.errors.email} />
 * <form onsubmit={form.handleSubmit((data) => save(data))}>
 * ```
 */
export interface FormApi<T extends Record<string, unknown>> {
	/** Reactive form values — bind fields to `form.values.<field>`. */
	readonly values: T;
	/** Reactive per-field errors (set on validate/submit failure). */
	readonly errors: Partial<Record<keyof T, string>>;
	/** True when there are no field errors. */
	readonly valid: boolean;
	/** True while the form's `onsubmit` is running. */
	readonly submitting: boolean;
	/** Validate all fields; returns parsed data on success, otherwise null. */
	validate(): T | null;
	/** Handle a native form submit: preventDefault, validate, then run `onSubmit` with the data. */
	handleSubmit(event: SubmitEvent, onSubmit: (data: T) => void | Promise<void>): Promise<void>;
	/** Clear a single field's error, or all errors when called without an argument. */
	clearErrors(field?: keyof T): void;
	/** Restore initial values and clear errors. */
	reset(): void;
	/** Internal — set the submitting flag around `onsubmit`. */
	setSubmitting(value: boolean): void;
}

/**
 * Create a reactive form bound to a Zod schema.
 * Superform-style: `values`, `errors`, `valid`, `submitting`, `validate()`,
 * `clearErrors()`, `reset()` — all readable outside the `<form>` element.
 */
export function createForm<T extends Record<string, unknown>>(
	schema: z.ZodType<T>,
	initial: T
): FormApi<T> {
	let values = $state<T>({ ...initial });
	let errors = $state<Partial<Record<keyof T, string>>>({});
	let submitting = $state(false);
	const valid = $derived(Object.values(errors).every((e) => !e));

	function validate(): T | null {
		const result = schema.safeParse(values);
		if (result.success) {
			errors = {};
			return result.data;
		}
		const next: Partial<Record<keyof T, string>> = {};
		for (const issue of result.error.issues) {
			const key = issue.path[0] as keyof T;
			if (key && !next[key]) next[key] = issue.message;
		}
		errors = next;
		return null;
	}

	async function handleSubmit(
		event: SubmitEvent,
		onSubmit: (data: T) => void | Promise<void>
	): Promise<void> {
		event.preventDefault();
		const data = validate();
		if (!data) return;
		submitting = true;
		try {
			await onSubmit(data);
		} finally {
			submitting = false;
		}
	}

	function clearErrors(field?: keyof T) {
		if (field === undefined) {
			errors = {};
			return;
		}
		if (errors[field]) {
			errors = { ...errors, [field]: undefined };
		}
	}

	function reset() {
		values = { ...initial } as T;
		errors = {};
	}

	return {
		get values() {
			return values;
		},
		get errors() {
			return errors;
		},
		get valid() {
			return valid;
		},
		get submitting() {
			return submitting;
		},
		validate,
		handleSubmit,
		clearErrors,
		reset,
		setSubmitting(v: boolean) {
			submitting = v;
		}
	};
}
