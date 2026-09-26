import { client } from '$lib/client';

/**
 * Shared import/restore logic for the Studio's Import and Backups pages.
 *
 * Both pages need the same things — parse a file, call `POST /api/import`, report
 * the result, handle the "large import needs confirmation" gate — so that logic
 * lives here instead of being copy-pasted. What stays page-specific is only the
 * presentation: the Import page's paste-a-JSON diff, the Backups page's
 * upload-a-file list.
 */

export type Preflight = {
	db_size_bytes: number;
	db_size_mb: number;
	threshold_mb: number;
	undo_available: boolean;
	neon_hosted: boolean;
};

export type ImportOutcome = {
	imported: { name: string; type?: string; records_imported?: number }[];
	errors: { name: string | null; error: string }[];
	rolled_back?: boolean;
};

export type ConfirmationRequired = {
	requires_confirmation: true;
	reason: string;
	message: string;
	preflight: Preflight;
};

export type AtomicMode = 'batch' | 'per_collection' | false;

export type ImportOptions = {
	deleteMissing: boolean;
	atomic: AtomicMode;
	password: string;
	confirm?: boolean;
};

export type ProgressFn = (percent: number) => void;

export const ACCEPTED_FILES = '.json,.zip';

/** Archives are detected by extension; the server sniffs the zip magic bytes. */
export function isArchive(file: File): boolean {
	return file.name.toLowerCase().endsWith('.zip');
}

export function parseCollections(text: string): {
	collections: Record<string, unknown>[] | null;
	error: string | null;
} {
	try {
		const data = JSON.parse(text);
		// Accept a bare array of collections and the backup envelope.
		const collections = Array.isArray(data) ? data : data?.collections;

		if (!Array.isArray(collections)) {
			return {
				collections: null,
				error: 'Invalid format. Expected an array of collections or { "collections": [...] }.'
			};
		}

		return { collections, error: null };
	} catch {
		return { collections: null, error: 'Invalid JSON format.' };
	}
}

/** The subset of `manifest.json` a preview needs. */
export type ArchiveManifest = {
	format?: string;
	format_version?: number;
	lazypock_version?: string;
	created_at?: string;
	collections?: { name: string; type?: string; records?: number }[];
	files?: { total?: number; bytes?: number; missing?: number };
	totals?: { collections?: number; records?: number; files?: number };
};

/**
 * Reads `manifest.json` out of an NDJSON archive **without uploading it**.
 *
 * The exporter always writes that entry first and records its real size in the
 * local file header (no data descriptor, no ZIP64), so only a few KB are read —
 * `File.slice` never loads the archive into memory. Uploading a multi-GB archive
 * twice (once to preview, once to import) would be worse than not previewing it,
 * which is why this is done in the browser rather than by a server endpoint.
 *
 * Returns null when the file is not a LazyPock archive or its zip layout cannot
 * be parsed cheaply; callers then simply import without a preview.
 */
export async function readArchiveManifest(file: File): Promise<ArchiveManifest | null> {
	try {
		if (file.size < 30) return null;

		const header = new DataView(await file.slice(0, 30).arrayBuffer());
		if (header.getUint32(0, true) !== 0x04034b50) return null;

		const flags = header.getUint16(6, true);
		const method = header.getUint16(8, true);
		const compressedSize = header.getUint32(18, true);
		const nameLength = header.getUint16(26, true);
		const extraLength = header.getUint16(28, true);

		// Bit 3: sizes live in a trailing data descriptor. 0xffffffff: ZIP64.
		// Either way the sizes cannot be trusted here, so give up quietly.
		if (flags & 0x08 || compressedSize === 0xffffffff) return null;

		const name = new TextDecoder().decode(
			new Uint8Array(await file.slice(30, 30 + nameLength).arrayBuffer())
		);
		if (name !== 'manifest.json') return null;

		const start = 30 + nameLength + extraLength;
		const raw = await file.slice(start, start + compressedSize).arrayBuffer();
		const json = method === 0 ? new TextDecoder().decode(raw) : await inflateRaw(raw);

		const manifest = JSON.parse(json) as ArchiveManifest;
		return manifest.format === 'lazypock-archive' ? manifest : null;
	} catch {
		return null;
	}
}

async function inflateRaw(buffer: ArrayBuffer): Promise<string> {
	if (typeof DecompressionStream === 'undefined') {
		throw new Error('DecompressionStream is unavailable in this browser');
	}

	const stream = new Blob([buffer]).stream().pipeThrough(new DecompressionStream('deflate-raw'));

	return new Response(stream).text();
}

/** Flattens a manifest into the `{name, type, recordCount}` rows the UIs render. */
export function manifestCollections(
	manifest: ArchiveManifest | null
): { name: string; type: string; recordCount: number }[] {
	return (manifest?.collections ?? []).map((c) => ({
		name: c.name,
		type: c.type || 'base',
		recordCount: c.records ?? 0
	}));
}

function authHeader(): Record<string, string> {
	const token = client.authStore.token;
	return token ? { Authorization: 'Bearer ' + token } : {};
}

/**
 * Error carrying the server's status + body so callers can tell the "large import
 * needs confirmation" gate apart from a real failure. Mirrors the SDK's
 * `ApiError` shape, which is why `confirmationRequired` handles both.
 */
export class ImportError extends Error {
	readonly status: number;
	readonly data: unknown;

	constructor(message: string, status: number, data: unknown) {
		super(message);
		this.name = 'ImportError';
		this.status = status;
		this.data = data;
	}
}

/** True when the server refused a large import pending explicit confirmation. */
export function confirmationRequired(err: unknown): ConfirmationRequired | null {
	if (!err || typeof err !== 'object') return null;

	const { status, data } = err as { status?: number; data?: unknown };
	if (status !== 409 || !data || typeof data !== 'object') return null;

	return (data as ConfirmationRequired).requires_confirmation
		? (data as ConfirmationRequired)
		: null;
}

export function describeError(err: unknown): string {
	if (err && typeof err === 'object' && 'message' in err) return String((err as Error).message);
	return String(err);
}

/**
 * Whether an automatic undo checkpoint is still available, so the UI can warn
 * BEFORE uploading a multi-GB archive. Returns null when the check itself fails
 * (the import still proceeds and the server enforces the gate either way).
 */
export async function getPreflight(): Promise<Preflight | null> {
	try {
		return (await client.http.get('/import/preflight')) as Preflight | null;
	} catch {
		return null;
	}
}

export async function importJson(
	opts: ImportOptions & { collections: unknown[] }
): Promise<ImportOutcome> {
	return (await client.http.post('/import', {
		collections: opts.collections,
		deleteMissing: opts.deleteMissing,
		atomic: opts.atomic,
		password: opts.password,
		...(opts.confirm ? { confirm: true } : {})
	})) as ImportOutcome;
}

/**
 * Uploads an archive via XHR rather than `fetch`, because `fetch` cannot report
 * upload progress and a multi-GB upload with no feedback is exactly the UX this
 * work is meant to fix. The bearer token is read from the SDK's own auth store so
 * it stays in sync with the rest of the app.
 */
export function uploadArchive(
	file: File,
	opts: ImportOptions,
	onProgress?: ProgressFn
): Promise<ImportOutcome> {
	return new Promise((resolve, reject) => {
		const form = new FormData();
		form.append('file', file, file.name);
		form.append('password', opts.password);
		form.append('deleteMissing', String(opts.deleteMissing));
		form.append('atomic', String(opts.atomic));
		if (opts.confirm) form.append('confirm', 'true');

		const xhr = new XMLHttpRequest();
		xhr.open('POST', '/api/import');
		for (const [key, value] of Object.entries(authHeader())) xhr.setRequestHeader(key, value);

		xhr.upload.onprogress = (event) => {
			if (event.lengthComputable && onProgress) {
				onProgress(Math.round((event.loaded / event.total) * 100));
			}
		};

		xhr.onload = () => {
			let data: unknown = null;
			try {
				data = xhr.responseText ? JSON.parse(xhr.responseText) : null;
			} catch {
				// Non-JSON error body — fall through to the status-based message.
			}

			if (xhr.status >= 200 && xhr.status < 300 && data) {
				resolve(data as ImportOutcome);
			} else {
				const body = data as { message?: string; error?: string } | null;

				reject(
					new ImportError(
						body?.message ?? body?.error ?? `Import failed (${xhr.status})`,
						xhr.status,
						data
					)
				);
			}
		};

		xhr.onerror = () => reject(new Error('Network error during upload'));
		xhr.onabort = () => reject(new Error('Upload cancelled'));
		xhr.send(form);
	});
}

/**
 * Downloads the streaming NDJSON archive with download progress. Unlike the JSON
 * export this never builds the whole database as a JS string.
 */
export function downloadArchive(onProgress?: ProgressFn): Promise<string> {
	return new Promise((resolve, reject) => {
		const xhr = new XMLHttpRequest();
		xhr.open('GET', '/api/export/archive');
		xhr.responseType = 'blob';
		for (const [key, value] of Object.entries(authHeader())) xhr.setRequestHeader(key, value);

		xhr.onprogress = (event) => {
			if (event.lengthComputable && onProgress) {
				onProgress(Math.round((event.loaded / event.total) * 100));
			}
		};

		xhr.onload = () => {
			if (xhr.status < 200 || xhr.status >= 300) {
				reject(new Error(`Export failed (${xhr.status})`));
				return;
			}

			const url = URL.createObjectURL(xhr.response as Blob);
			const anchor = document.createElement('a');
			anchor.href = url;
			anchor.download = `lazypock-backup-${new Date().toISOString().slice(0, 10)}.zip`;
			anchor.click();
			URL.revokeObjectURL(url);
			resolve(anchor.download);
		};

		xhr.onerror = () => reject(new Error('Network error during export'));
		xhr.send();
	});
}

export function summarize(
	outcome: ImportOutcome,
	verb = 'Imported'
): { ok: boolean; message: string } {
	const imported = outcome.imported?.length ?? 0;
	const errors = outcome.errors?.length ?? 0;

	if (outcome.rolled_back) {
		return {
			ok: false,
			message: `${verb} failed — all changes were rolled back (${errors} errors)`
		};
	}

	if (errors > 0) {
		return { ok: false, message: `${verb} ${imported} collections with ${errors} errors` };
	}

	return { ok: true, message: `${verb} ${imported} collections` };
}
