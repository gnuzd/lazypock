// ── Schema types ─────────────────────────────────────────
// Shared runtime + codegen types for describing collections/fields.

/** A single field definition as returned by `GET /collections`. */
export interface SchemaField {
	id?: string;
	name: string;
	type: string;
	required?: boolean;
	unique?: boolean;
	options?: {
		/** For relation fields: the target collection name. */
		collection?: string;
		/** Max selectable/related items (multi when > 1). */
		maxSelect?: number;
		/** Allowed values for select / multi_select fields. */
		values?: string[];
	} & Record<string, unknown>;
	indexed?: boolean;
	hidden?: boolean;
	system?: boolean;
	sort_order?: number;
}

/** A collection definition as returned by `GET /collections`. */
export interface CollectionSchema {
	id?: string;
	name: string;
	type: "base" | "auth";
	system?: boolean;
	fields?: SchemaField[];
	rules?: Record<string, unknown>;
	options?: Record<string, unknown>;
}

/** Shape of the `GET /collections` response. */
export interface CollectionsResponse {
	items: CollectionSchema[];
}
