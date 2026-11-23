import { z } from 'zod';

// ---------- Types ----------
export type FormErrors<T> = Partial<Record<keyof T, string>> & { _form?: string };

export type Constraint = {
  required?: boolean;
  minlength?: number;
  maxlength?: number;
  min?: number;
  max?: number;
  pattern?: string;
};

export type SubmitResult<T> = {
  success: boolean;
  errors?: FormErrors<T>;
  data?: Partial<T>;
  message?: string;
};

export type SubmitHandler<T> = (
  values: T
) => Promise<SubmitResult<T> | void> | SubmitResult<T> | void;

// ---------- Zod → FormErrors ----------
export function formatZodError<T>(error: z.ZodError): FormErrors<T> {
  // FIX TS LỖI: Ép kiểu chuẩn xác cho kết quả trả về của Zod
  const flat = z.flattenError(error) as {
    formErrors: string[];
    fieldErrors: Record<string, string[] | undefined>;
  };
  
  const out: FormErrors<T> = {};

  if (flat.formErrors.length > 0) {
    out._form = flat.formErrors[0];
  }

  for (const [key, msgs] of Object.entries(flat.fieldErrors)) {
    if (msgs && msgs.length > 0) {
      (out as Record<string, string>)[key] = msgs[0];
    }
  }

  return out;
}

// ---------- Zod → HTML Constraints ----------
export function extractConstraints<S extends z.ZodObject<z.ZodRawShape>>(
  schema: S
): Partial<Record<keyof z.infer<S>, Constraint>> {
  const out: Record<string, Constraint> = {};
  const shape = schema.shape as Record<string, z.ZodTypeAny>;

  for (const [key, field] of Object.entries(shape)) {
    const c: Constraint = { required: !(field instanceof z.ZodOptional) };

    let current: z.ZodTypeAny = field;
    while (typeof (current as any).unwrap === 'function') {
      current = (current as any).unwrap();
    }

    const checks = getChecks(current);
    for (const chk of checks) {
      const kind = chk.kind ?? chk._zod?.def?.check;
      const value = chk.value ?? chk._zod?.def?.value;

      if (kind === 'min') {
        c.min = value;
        c.minlength = value;
      } else if (kind === 'max') {
        c.max = value;
        c.maxlength = value;
      } else if (kind === 'regex') {
        const regex = chk.regex ?? chk._zod?.def?.regex;
        if (regex) c.pattern = regex.source;
      }
    }

    out[key] = c;
  }

  return out as Partial<Record<keyof z.infer<S>, Constraint>>;
}

function getChecks(field: z.ZodTypeAny): any[] {
  const f = field as any;
  return f._zod?.def?.checks ?? f._def?.checks ?? [];
}

// ---------- FormData → object ----------
export function formDataToObject<S extends z.ZodObject<z.ZodRawShape>>(
  schema: S,
  formData: FormData
): Partial<z.infer<S>> {
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(schema.shape)) {
    const v = formData.get(key);
    if (v !== null) out[key] = v;
  }
  return out as Partial<z.infer<S>>;
}
