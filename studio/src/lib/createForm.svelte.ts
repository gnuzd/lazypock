import { applyAction, deserialize } from '$app/forms';
import { invalidateAll } from '$app/navigation';
import type { ActionResult } from '@sveltejs/kit';
import { z } from 'zod';

type Errors<T> = Partial<Record<keyof T, string>> & { _form?: string };
type Bools<T> = Partial<Record<keyof T, boolean>>;
type Constraint = {
	required?: boolean;
	minlength?: number;
	maxlength?: number;
	min?: number;
	max?: number;
	pattern?: string;
};
type Constraints<T> = Partial<Record<keyof T, Constraint>>;
type AsyncValidators<T> = Partial<{
	[K in keyof T]: (value: T[K], values: T) => Promise<string | void>;
}>;
type SubmitResult<T> = {
	success: boolean;
	errors?: Errors<T>;
	data?: any;
	message?: string;
};
type SubmitFn<T> = (values: T) => Promise<SubmitResult<T> | void>;

function buildConstraints<S extends z.ZodObject<any>>(schema: S): Constraints<z.infer<S>> {
	const constraints: Record<string, Constraint> = {};

	for (const [key, fieldSchemaRaw] of Object.entries(schema.shape)) {
		const fieldSchema = fieldSchemaRaw as any; // cắt type-check ở đây
		let def = fieldSchema;
		const c: Constraint = { required: !def.isOptional?.() };

		while (def?._def?.typeName === 'ZodOptional' || def?._def?.typeName === 'ZodNullable') {
			def = def.unwrap();
		}

		const checks = def?._def?.checks ?? [];
		for (const check of checks) {
			if (check.kind === 'min') {
				c.minlength = check.value;
				c.min = check.value;
			}
			if (check.kind === 'max') {
				c.maxlength = check.value;
				c.max = check.value;
			}
			if (check.kind === 'regex') c.pattern = check.regex?.source;
		}

		constraints[key] = c;
	}
	return constraints as Constraints<z.infer<S>>;
}

export function createForm<S extends z.ZodObject<any>>(
	schema: S,
	initial: z.infer<S>,
	asyncValidators: AsyncValidators<z.infer<S>> = {}
) {
	type T = z.infer<S>;

	let form = $state<T>({ ...(initial as object) } as T);
	let errors = $state<Errors<T>>({});
	let touched = $state<Bools<T>>({});
	let tainted = $state<Bools<T>>({}); // = "dirty" trong Superforms
	let validating = $state<Bools<T>>({});
	let submitting = $state(false);
	let delayed = $state(false); // true nếu submitting kéo dài > 300ms
	let message = $state<string | undefined>(undefined);

	const constraints = buildConstraints(schema);
	const isValid = $derived(Object.values(errors).every((e) => !e));
	const isTainted = $derived(Object.values(tainted).some(Boolean));

	function validateAll() {
		const result = schema.safeParse(form);
		const next: Errors<T> = {};
		if (!result.success) {
			for (const issue of result.error.issues) {
				const key = issue.path[0] as keyof T | undefined;
				if (key === undefined) next._form = issue.message;
				else if (!next[key]) (next as Record<string, string>)[key as string] = issue.message;
			}
		}
		errors = next;
		return result.success;
	}

	async function validateField(key: keyof T) {
		touched[key] = true;

		tainted[key] = JSON.stringify(form[key]) !== JSON.stringify(initial[key]);

		const fieldSchema = schema.shape[key as string];
		const result = fieldSchema?.safeParse(form[key]);
		let msg = result && !result.success ? result.error.issues[0]?.message : undefined;

		const asyncFn = asyncValidators[key];
		if (asyncFn && !msg) {
			validating[key] = true;
			try {
				msg = (await asyncFn(form[key], form)) || undefined;
			} finally {
				validating[key] = false;
			}
		}

		errors[key] = msg;
	}

	function reset() {
		form = { ...(initial as object) } as T;
		errors = {};
		touched = {};
		tainted = {};
		message = undefined;
	}

	function enhance(formEl: HTMLFormElement, onSubmit?: SubmitFn<T>) {
		async function handleSubmit(e: SubmitEvent) {
			e.preventDefault();

			for (const key of Object.keys(form) as (keyof T)[]) {
				touched[key] = true;
			}
			if (!validateAll()) return;

			submitting = true;
			const delayTimer = setTimeout(() => (delayed = true), 300);

			try {
				if (onSubmit) {
					const result = await onSubmit(form);
					if (result && !result.success && result.errors) errors = result.errors;
					else if (result && result.success) tainted = {};
					if (result?.message) message = result.message;
				} else {
					const res = await fetch(formEl.action, {
						method: formEl.method || 'POST',
						headers: { accept: 'application/json' },
						body: new FormData(formEl)
					});

					const result: ActionResult = deserialize(await res.text());

					if (result.type === 'failure') {
						const failErrors = result.data?.errors;
						if (failErrors) errors = failErrors as Errors<T>;
					} else if (result.type === 'success') {
						const successForm = result.data?.form;
						if (successForm) form = { ...form, ...successForm };
						tainted = {};
						await invalidateAll();
					}
					if (result.type !== 'error' && (result as any).data?.message) {
						message = (result as any).data.message;
					}

					applyAction(result);
				}
			} finally {
				clearTimeout(delayTimer);
				submitting = false;
				delayed = false;
			}
		}

		formEl.addEventListener('submit', handleSubmit);
		return {
			destroy: () => formEl.removeEventListener('submit', handleSubmit)
		};
	}

	return {
		get form() {
			return form;
		},
		set form(v) {
			form = v;
		},
		get errors() {
			return errors;
		},
		get touched() {
			return touched;
		},
		get tainted() {
			return tainted;
		},
		get validating() {
			return validating;
		},
		get submitting() {
			return submitting;
		},
		get delayed() {
			return delayed;
		},
		get message() {
			return message;
		},
		set message(v) {
			message = v;
		},
		constraints,
		get isValid() {
			return isValid;
		},
		get isTainted() {
			return isTainted;
		},
		validateField,
		validateAll,
		reset,
		enhance
	};
}
