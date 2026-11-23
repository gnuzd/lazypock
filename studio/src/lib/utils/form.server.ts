import type { z } from 'zod';
import { formatZodError, formDataToObject, type FormErrors } from './form';

export type ValidateResult<T> =
  | { success: true; data: T }
  | { success: false; errors: FormErrors<T> };

export async function validate<S extends z.ZodObject<z.ZodRawShape>>(
  schema: S,
  data: unknown
): Promise<ValidateResult<z.infer<S>>> {
  const result = await schema.safeParseAsync(data);
  if (result.success) return { success: true, data: result.data };
  return { success: false, errors: formatZodError<z.infer<S>>(result.error) };
}

export { formDataToObject };
