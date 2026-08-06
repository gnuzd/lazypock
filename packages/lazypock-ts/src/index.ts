// ── Lazypock SDK — Public Entry ────────────────────────
//
// Re-exports everything from the LazypockClient module plus the
// typed-client factory. Keep this file a thin barrel so there are
// no circular imports.

export {
	// LazypockClient + options + everything it re-exports
	AuthStore,
	ApiError,
	RealtimeService,
	wsUrlFromBaseUrl,
	FilesService,
	getFileUrl,
	getThumbUrl,
	getScaleUrl,
	LazypockClient,
	CollectionService,
	CollectionsService,
	generateTypes,
	collectionTypeName,
	fieldTypeScriptType,
	fieldTypeKind,
	schemaFieldType,
} from "./lazypock";
export { TypedClient, createClient } from "./client";

export type {
	// types
	StorageAdapter,
	AuthModel,
	ApiRecord,
	ListResult,
	RecordShape,
	CreateData,
	UpdateData,
	SystemFields,
	RequestOptions,
	FileRecord,
	// schema
	CollectionSchema,
	SchemaField,
	// options
	LazypockClientOptions,
} from "./lazypock";
export type { LazypockCollections } from "./client";
export type { RealtimeMessage, RealtimeCallback } from "./collection";
export type { CollectionsMessage } from "./collections";
