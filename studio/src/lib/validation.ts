import { z } from 'zod';
import type { FieldDefinition } from './fieldTypes';

// ── Login Form ──────────────────────────────────────────

export const loginSchema = z.object({
	email: z.string().email('Invalid email address'),
	password: z.string().min(8, 'Password must be at least 8 characters')
});

export type LoginData = z.infer<typeof loginSchema>;

export const setupSchema = z
	.object({
		email: z.string().email('Invalid email address'),
		password: z.string().min(8, 'Password must be at least 8 characters'),
		confirmPassword: z.string().min(1, 'Please confirm your password')
	})
	.refine((data) => data.password === data.confirmPassword, {
		message: 'Passwords do not match',
		path: ['confirmPassword']
	});

export type SetupData = z.infer<typeof setupSchema>;

// ── Collection Form ─────────────────────────────────────

const collectionFieldSchema = z.object({
	name: z.string().min(1, 'Field name is required'),
	type: z.string().min(1, 'Field type is required'),
	system: z.boolean().optional(),
	hidden: z.boolean().optional(),
	required: z.boolean().optional(),
	unique: z.boolean().optional(),
	indexed: z.boolean().optional(),
	presentable: z.boolean().optional(),
	options: z.record(z.string(), z.unknown()).optional()
});

export const collectionSchema = z.object({
	name: z
		.string()
		.min(1, 'Collection name is required')
		.regex(/^[a-z0-9_]+$/, 'Only lowercase letters, numbers, and underscores'),
	type: z.enum(['base', 'view', 'auth']),
	indexes: z.array(z.string()).optional(),
	fields: z.array(collectionFieldSchema),
	listRule: z.string().nullable().optional(),
	viewRule: z.string().nullable().optional(),
	createRule: z.string().nullable().optional(),
	updateRule: z.string().nullable().optional(),
	deleteRule: z.string().nullable().optional()
});

export type CollectionFormData = z.infer<typeof collectionSchema>;

// ── Record Form (dynamic — built from collection schema) ─

const NON_TEXT_TYPES = new Set(['number', 'bool', 'date', 'datetime']);

/**
 * Build a zod schema for a record form from the collection's field definitions.
 * Returns `{ schema, fieldsMap }` where `schema` validates the whole form object
 * and `fieldsMap` maps field names to their type for display purposes.
 */
export function buildRecordSchema(
	fields: Record<string, unknown>[],
	opts?: { isCreate?: boolean }
) {
	const shape: Record<string, z.ZodTypeAny> = {};

	const fieldsMap: Record<string, string> = {};
	const isCreate = opts?.isCreate ?? true;

	// Skip auto-managed fields — set by backend automatically
	for (const f of fields) {
		if ((f as FieldDefinition).type === 'autodate') continue;
		if ((f as FieldDefinition).name === 'id') continue;
		const name = f.name as string;
		const type = f.type as string;
		const required = !!(f as FieldDefinition).required;

		fieldsMap[name] = type;

		switch (type) {
			case 'number': {
				if (required) {
					// Accept number or string-that-converts, require non-null after transform
					shape[name] = z
						.union([z.number(), z.string().transform((v) => (v === '' ? undefined : Number(v)))])
						.pipe(z.union([z.number(), z.undefined()]))
						.refine((v) => v !== undefined, { message: 'Required' });
				} else {
					shape[name] = z
						.union([z.number(), z.string().transform((v) => (v === '' ? undefined : Number(v)))])
						.nullable()
						.optional();
				}
				break;
			}

			case 'bool':
				shape[name] = z.boolean().optional().default(false);
				break;

			case 'email':
				if (required) {
					shape[name] = z.string().email('Invalid email').min(1, 'Required');
				} else {
					shape[name] = z.string().email('Invalid email').nullable().optional().or(z.literal(''));
				}
				break;

			case 'url':
				if (required) {
					shape[name] = z.string().url('Invalid URL').min(1, 'Required');
				} else {
					shape[name] = z.string().url('Invalid URL').nullable().optional().or(z.literal(''));
				}
				break;

			case 'date':
			case 'datetime':
				if (required) {
					shape[name] = z.string().min(1, 'Required');
				} else {
					shape[name] = z.string().nullable().optional();
				}
				break;

			case 'json':
			case 'geo':
				if (required) {
					shape[name] = z
						.record(z.string(), z.unknown())
						.refine((v) => v !== null && v !== undefined, { message: 'Required' });
				} else {
					shape[name] = z.record(z.string(), z.unknown()).nullable().optional();
				}
				break;

			case 'select':
				if (required) {
					shape[name] = z.string().min(1, 'Required');
				} else {
					shape[name] = z.string().nullable().optional();
				}
				break;

			case 'multi_select':
				if (required) {
					shape[name] = z.array(z.string()).min(1, 'Required');
				} else {
					shape[name] = z.array(z.string()).optional();
				}
				break;

			case 'relation':
				if (required) {
					shape[name] = z.string().min(1, 'Required');
				} else {
					shape[name] = z.string().nullable().optional();
				}
				break;

			default:
				// text, editor, password, file
				if (type === 'password') {
					// Create: a required password must be set. Edit: empty = keep existing.
					if (isCreate && required) {
						shape[name] = z
							.string()
							.min(8, 'Password must be at least 8 characters');
					} else {
						shape[name] = z.string().nullable().optional();
					}
				} else if (required) {
					shape[name] = z.string().min(1, 'Required');
				} else {
					shape[name] = z.string().nullable().optional();
				}
				break;
		}
	}

	return { schema: z.object(shape), fieldsMap };
}

/**
 * Clean record data before sending to API: strip system fields,
 * convert empty strings to null for non-text types.
 */
export function cleanRecordData(
	data: Record<string, unknown>,
	fields: Record<string, unknown>[]
): Record<string, unknown> {
	const systemFields = new Set(['id', 'created_at', 'updated_at', 'collectionName']);
	const cleaned: Record<string, unknown> = {};

	for (const key of Object.keys(data)) {
		if (systemFields.has(key)) continue;
		const fieldDef = fields.find((f) => f.name === key);
		const type = (fieldDef?.type as string) ?? 'text';
		const val = data[key];

		if (val === '' && type === 'password') {
			// Empty password = keep existing (skip entirely)
			continue;
		} else if (val === '' && NON_TEXT_TYPES.has(type)) {
			cleaned[key] = null;
		} else {
			cleaned[key] = val;
		}
	}

	return cleaned;
}
