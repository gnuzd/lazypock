// ── File Service ─────────────────────────────────────────
// Upload, download, and delete files.

import type { HttpClient } from "./http";
import type { RequestOptions } from "./types";

/** Response shape from the server file endpoints */
export interface FileRecord {
	id: string;
	filename: string;
	mimeType: string;
	size: number;
	url: string;
	/** Map of thumbnail size => URL, e.g. { "50x50": "/api/files/<id>/thumbs/50x50" } */
	thumbs?: Record<string, string>;
	[key: string]: unknown;
}

/**
 * Construct a file URL from the API base URL and file ID.
 */
export function getFileUrl(baseUrl: string, fileId: string): string {
	return baseUrl.replace(/\/+$/, "") + "/files/" + encodeURIComponent(fileId);
}

/**
 * Construct a thumbnail URL from the API base URL, file ID, and thumb size.
 * @param size e.g. "50x50"
 */
export function getThumbUrl(baseUrl: string, fileId: string, size: string): string {
	return (
		baseUrl.replace(/\/+$/, "") +
		"/files/" +
		encodeURIComponent(fileId) +
		"/thumbs/" +
		encodeURIComponent(size)
	);
}

/**
 * Construct an on-demand scaled image URL from the API base URL, file ID, and size.
 *
 * The size is an ImageMagick geometry: "100" (width, keep aspect), "100x100"
 * (fit within box), "100x100!" (exact crop), "x200" (height). The server
 * generates and caches the scaled image on first request.
 * @param size e.g. "100x100"
 */
export function getScaleUrl(baseUrl: string, fileId: string, size: string): string {
	return (
		baseUrl.replace(/\/+$/, "") +
		"/files/" +
		encodeURIComponent(fileId) +
		"/scale/" +
		encodeURIComponent(size)
	);
}


/**
 * Service for file upload, retrieval, and deletion.
 * Access via {@link LazypockClient.files}.
 */
export class FilesService {
	constructor(private http: HttpClient) {}

	/**
	 * Upload a file or blob.
	 *
	 * @param file The File or Blob to upload.
	 * @param filename Optional filename (required if `file` is a Blob without a name).
	 * @param options Optional request options (signal, custom fetch).
	 * @param meta Optional metadata: collectionName, recordId, fieldName for ownership tracking.
	 */
	async upload(
		file: File | Blob,
		filename?: string,
		options?: RequestOptions,
		meta?: { collectionName?: string; recordId?: string; fieldName?: string },
	): Promise<FileRecord | null> {
		if (typeof FormData === "undefined") {
			throw new Error("FormData is not available in this environment");
		}

		const formData = new FormData();
		const name = filename || (file instanceof File ? file.name : "file");
		formData.append("file", file, name);

		if (meta?.collectionName)
			formData.append("collection_name", meta.collectionName);
		if (meta?.recordId) formData.append("record_id", meta.recordId);
		if (meta?.fieldName) formData.append("field_name", meta.fieldName);

		const data = await this.http.request<Record<string, unknown>>(
			"POST",
			"/files",
			formData,
			options,
		);

		return data as FileRecord | null;
	}

	/**
	 * List uploaded files (newest first), with optional filters.
	 *
	 * @param options Filters and pagination.
	 */
	async list(options?: {
		page?: number;
		perPage?: number;
		collectionName?: string;
		fieldName?: string;
		mime?: string;
	}): Promise<{ items: FileRecord[]; page: number; perPage: number; total: number }> {
		const params: Record<string, string> = {};
		if (options?.page !== undefined) params["page"] = String(options.page);
		if (options?.perPage !== undefined) params["perPage"] = String(options.perPage);
		if (options?.collectionName) params["collectionName"] = options.collectionName;
		if (options?.fieldName) params["fieldName"] = options.fieldName;
		if (options?.mime) params["mime"] = options.mime;

		const data = await this.http.request<{
			items: FileRecord[];
			page: number;
			perPage: number;
			total: number;
		}>("GET", "/files", undefined, { params });
		return (
			data ?? { items: [], page: 1, perPage: 50, total: 0 }
		) as {
			items: FileRecord[];
			page: number;
			perPage: number;
			total: number;
		};
	}

	/**
	 * Fetch file metadata including URL.
	 * @param fileId The file ID.
	 */

	/**
	 * Fetch file metadata including URL.
	 * @param fileId The file ID.
	 */
	async getUrl(fileId: string): Promise<string | null> {
		const data = await this.http.request<Record<string, unknown>>(
			"GET",
			"/files/" + encodeURIComponent(fileId),
		);
		if (data && typeof data === "object" && "url" in data) {
			return (data as Record<string, unknown>).url as string;
		}
		return null;
	}

	/**
	 * Delete a file by ID.
	 * @param fileId The file ID.
	 * @param options Optional request options.
	 */
	async delete(fileId: string, options?: RequestOptions): Promise<null> {
		return this.http.request<null>(
			"DELETE",
			"/files/" + encodeURIComponent(fileId),
			undefined,
			options,
		);
	}
}
