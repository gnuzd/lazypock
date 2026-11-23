import type { z } from 'zod';
import {
  formatZodError,
  extractConstraints,
  type Constraint,
  type FormErrors,
  type SubmitHandler
} from './form';

export function createForm<S extends z.ZodObject<z.ZodRawShape>>(
  schema: S,
  initialData: z.infer<S>,
  options: { onSubmit?: SubmitHandler<z.infer<S>> } = {}
) {
  type Values = z.infer<S>;
  type Keys = keyof Values;

  // ---------- State ----------
  let form = $state<Values>({ ...initialData });
  let errors = $state<FormErrors<Values>>({});
  let touched = $state<Partial<Record<Keys, boolean>>>({});
  let tainted = $state<Partial<Record<Keys, boolean>>>({});
  let submitting = $state(false);
  let delayed = $state(false);
  let message = $state<string | undefined>();

  // ---------- Derived ----------
  const isValid = $derived(Object.values(errors).every((e) => !e));
  const isTainted = $derived(Object.values(tainted).some(Boolean));
  const constraints = extractConstraints(schema);

  // ---------- Validation ----------
  async function validateAll(): Promise<boolean> {
    const result = await schema.safeParseAsync(form);
    if (result.success) {
      errors = {};
      return true;
    }
    errors = formatZodError<Values>(result.error);
    return false;
  }

  async function validateField(key: Keys): Promise<void> {
    touched[key] = true;
    tainted[key] = JSON.stringify(form[key]) !== JSON.stringify(initialData[key]);

    const fieldSchema = (schema.shape as Record<string, z.ZodTypeAny>)[key as string];
    if (!fieldSchema) return;

    const result = await fieldSchema.safeParseAsync(form[key]);
    if (result.success) {
      (errors as Record<string, string | undefined>)[key as string] = undefined;
      return;
    }
    (errors as Record<string, string | undefined>)[key as string] =
      result.error.issues[0]?.message;
  }

  // ---------- Actions ----------
  function reset(): void {
    form = { ...initialData };
    errors = {};
    touched = {};
    tainted = {};
    message = undefined;
  }

  function setError(key: Keys, msg: string): void {
    (errors as Record<string, string>)[key as string] = msg;
  }

  function setMessage(msg: string): void {
    message = msg;
  }

  // ---------- enhance (Svelte action) ----------
  function enhance(formEl: HTMLFormElement, onSubmit?: SubmitHandler<Values>) {
    async function handleSubmit(e: SubmitEvent): Promise<void> {
      e.preventDefault();

      for (const key of Object.keys(form) as Keys[]) touched[key] = true;

      const ok = await validateAll();
      if (!ok) return;

      submitting = true;
      const timer = setTimeout(() => (delayed = true), 300);

      try {
        const handler = onSubmit ?? options.onSubmit;

        if (handler) {
          const result = await handler(form);
          if (result && !result.success && result.errors) errors = result.errors;
          else if (result && result.success) {
            tainted = {};
            if (result.data) form = { ...form, ...result.data };
          }
          if (result?.message) message = result.message;
        } else {
          // Fallback: POST formEl.action, expect JSON
          // support both SvelteKit ActionResult and shape { success, errors, data, message }
          const res = await fetch(formEl.action, {
            method: formEl.method || 'POST',
            headers: { accept: 'application/json' },
            body: new FormData(formEl)
          });
          const json: any = await res.json().catch(() => null);
          if (!json) return;

          if (json.type === 'failure' && json.data?.errors) {
            errors = json.data.errors;
          } else if (json.type === 'success') {
            if (json.data?.form) form = { ...form, ...json.data.form };
            tainted = {};
            if (json.data?.message) message = json.data.message;
          } else if (json.success !== undefined) {
            if (!json.success && json.errors) errors = json.errors;
            else if (json.success) {
              tainted = {};
              if (json.data) form = { ...form, ...json.data };
            }
            if (json.message) message = json.message;
          }
        }
      } finally {
        clearTimeout(timer);
        submitting = false;
        delayed = false;
      }
    }

    formEl.addEventListener('submit', handleSubmit);
    return { destroy: () => formEl.removeEventListener('submit', handleSubmit) };
  }

  // ---------- Return ----------
  return {
    get form() { return form; },
    set form(v: Values) { form = v; },

    get errors() { return errors; },
    get touched() { return touched; },
    get tainted() { return tainted; },
    get submitting() { return submitting; },
    get delayed() { return delayed; },
    get message() { return message; },
    set message(v: string | undefined) { message = v; },

    get isValid() { return isValid; },
    get isTainted() { return isTainted; },

    constraints,
    validateAll,
    validateField,
    reset,
    setError,
    setMessage,
    enhance
  };
}
