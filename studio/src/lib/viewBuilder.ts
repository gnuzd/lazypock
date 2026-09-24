/**
 * Types + defaults for the no-code view builder.
 *
 * The spec is stored server-side in the view collection's
 * `options["view_builder"]` and turned into SQL by the backend
 * (`Lazypock.Schema.ViewBuilder`). See
 * `POST /api/collections/meta/preview-view-builder` for the live preview.
 */

export interface ViewBuilderField {
	/** Base collection name, or a declared relation alias. */
	source: string;
	/** Column name on the source/target collection. */
	name: string;
	/** Output column name; omitted means the server default. */
	as?: string;
}

export interface ViewBuilderRelation {
	/** Stable alias (t1, t2, …) referenced by `fields[].source`. */
	alias: string;
	/** The relation field on the base collection. */
	field: string;
}

export interface ViewBuilderSpec {
	source: string;
	relations: ViewBuilderRelation[];
	fields: ViewBuilderField[];
	/** PocketBase-style sort over base columns (e.g. "-created_at, title"). */
	sort?: string;
	limit?: number;
}

export function emptyViewBuilderSpec(): ViewBuilderSpec {
	return { source: '', relations: [], fields: [] };
}
