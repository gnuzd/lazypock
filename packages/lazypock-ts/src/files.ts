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
	[key: string]: unknown;
}

/**
 * Construct a file URL from the API base URL and file ID.
 */
export function getFileUrl(baseUrl: string, fileId: string): string {
	return baseUrl.replace(/\/+$/, "") + "/files/" + encodeURIComponent(fileId);
}

export class FilesService {
	constructor(private http: HttpClient) {}

	/**
	 * Upload a file or blob.
	 *
	 * @param file   The File or Blob to upload.
	 * @param filename  Optional filename (required if file is a Blob).
	 * @param options   Optional request options (signal, custom fetch).
	 * @param meta      Optional metadata: collectionName, recordId, fieldName.
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
	 * GET /api/files/:id — returns the file metadata.
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
	 * DELETE /api/files/:id
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
