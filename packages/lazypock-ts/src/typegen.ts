// ── Type mapping (runtime + codegen) ────────────────────
// Maps server field types to TypeScript types.
// Shared by the runtime `SchemaTypes` helper and the codegen CLI.

import type { SchemaField } from "./schema";

/**
 * Map a single server field to its TypeScript type string.
 * Used by the codegen CLI to emit interface members.
 *
 * @param field The field definition.
 * @param fallback Fallback type for unknown field types (default `unknown`).
 */
export function fieldTypeScriptType(
	field: SchemaField,
	fallback = "unknown",
): string {
	const opts = field.options ?? {};
	switch (field.type) {
		case "text":
		case "email":
		case "url":
		case "editor":
		case "date":
		case "datetime":
			return "string";
		case "number":
			return "number";
		case "bool":
			return "boolean";
		case "select": {
			const values = Array.isArray(opts.values) ? opts.values : [];
			if (values.length > 0) {
				return values.map((v) => JSON.stringify(String(v))).join(" | ");
			}
			return "string";
		}
		case "multi_select": {
			const values = Array.isArray(opts.values) ? opts.values : [];
			if (values.length > 0) {
				return `(${values.map((v) => JSON.stringify(String(v))).join(" | ")})[]`;
			}
			return "string[]";
		}
		case "file":
			return "string";
		case "multi_file":
			return "string[]";
		case "json":
		case "geo":
			return "Record<string, unknown>";
		case "relation":
			// Relations store the target record's ID (string) — or an array
			// of IDs when multi-relation (maxSelect > 1).
			return (opts.maxSelect ?? 1) > 1 ? "string[]" : "string";
		case "password":
			// Passwords are write-only; never expose on read models.
			return "never";
		default:
			return fallback;
	}
}

/**
 * Returns the runtime type kind for a field — used by {@link schemaFieldType}
 * to build structural types at runtime.
 */
export type FieldTypeKind =
	| "string"
	| "number"
	| "boolean"
	| "string-array"
	| "json"
	| "relation"
	| "relation-many"
	| "password"
	| "unknown";

/** Map a server field to its runtime type kind. */
export function fieldTypeKind(field: SchemaField): FieldTypeKind {
	const opts = field.options ?? {};
	switch (field.type) {
		case "text":
		case "email":
		case "url":
		case "editor":
		case "date":
		case "datetime":
		case "select":
		case "file":
			return "string";
		case "number":
			return "number";
		case "bool":
			return "boolean";
		case "multi_select":
		case "multi_file":
			return "string-array";
		case "json":
		case "geo":
			return "json";
		case "relation":
			return (opts.maxSelect ?? 1) > 1 ? "relation-many" : "relation";
		case "password":
			return "password";
		default:
			return "unknown";
	}
}

/**
 * Derive a TypeScript field type from a {@link SchemaField} — the runtime
 * counterpart to the codegen mapper. Lets consumers build typed clients
 * from a fetched schema without running the CLI.
 */
export function schemaFieldType(field: SchemaField): unknown {
	switch (fieldTypeKind(field)) {
		case "string":
			return String;
		case "number":
			return Number;
		case "boolean":
			return Boolean;
		case "string-array":
			return [String] as const;
		case "relation":
			return String;
		case "relation-many":
			return [String] as const;
		case "json":
			return Object;
		case "password":
			return undefined;
		case "unknown":
			return undefined;
	}
}
