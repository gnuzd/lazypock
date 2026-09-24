/**
 * Builds a self-contained prompt that a user can paste into Gemini / ChatGPT /
 * Claude to generate a Lazypock import file. It bundles the format rules, a
 * worked example and the instance's current collections, so the assistant does
 * not need to know anything about Lazypock up front (and works even without
 * web browsing). Keep this in sync with https://lazypock.gnuzd.dev/backup.
 */

export interface PromptField {
	name: string;
	type: string;
	required?: boolean;
	unique?: boolean;
	hidden?: boolean;
	system?: boolean;
	options?: Record<string, unknown>;
}

export interface PromptCollection {
	name: string;
	type?: string;
	fields?: PromptField[];
}

/** Canonical backend field types (see Lazypock.Schema.TypeMapper). */
const FIELD_TYPES = [
	'text',
	'editor',
	'number',
	'bool',
	'email',
	'url',
	'date',
	'datetime',
	'autodate',
	'select',
	'multi_select',
	'file',
	'multi_file',
	'relation',
	'json',
	'geo',
	'password'
];

const EXAMPLE = `{
  "collections": [
    {
      "name": "posts",
      "type": "base",
      "schema": [
        { "name": "title", "type": "text", "required": true },
        {
          "name": "status",
          "type": "select",
          "options": { "values": ["draft", "published"], "maxSelect": 1 }
        },
        {
          "name": "author",
          "type": "relation",
          "options": { "collection": "users", "maxSelect": 1 }
        }
      ],
      "rules": {
        "listRule": "",
        "viewRule": "",
        "createRule": "",
        "updateRule": "",
        "deleteRule": ""
      },
      "options": { "indexes": [] },
      "records": [
        {
          "id": "f0b1c2d3-e4f5-4a6b-8c7d-9e0f1a2b3c4d",
          "title": "Hello world",
          "status": "published"
        }
      ]
    }
  ]
}`;

function describeField(f: PromptField): string {
	const flags = [
		f.required ? 'required' : '',
		f.unique ? 'unique' : '',
		f.hidden ? 'hidden' : ''
	].filter(Boolean);
	const opts =
		f.options && Object.keys(f.options).length > 0 ? ` options=${JSON.stringify(f.options)}` : '';
	return `${f.name}: ${f.type}${flags.length ? ` (${flags.join(', ')})` : ''}${opts}`;
}

/** Compact "current schema" section, system fields filtered out. */
function describeCollections(collections: PromptCollection[]): string {
	if (collections.length === 0) return '  (the database has no collections yet)';

	return collections
		.map((c) => {
			const fields = (c.fields ?? []).filter((f) => !f.system).map((f) => describeField(f));
			const header = `  - ${c.name} (${c.type || 'base'})`;
			const body = fields.length ? `${fields.map((f) => `\n      ${f}`).join('')}` : ' — no fields';
			return header + body;
		})
		.join('\n');
}

export function buildImportPrompt(collections: PromptCollection[]): string {
	return `You are generating a Lazypock import file (JSON) that I will paste into the Lazypock Studio (Settings → Import) or restore via \`POST /api/import\`.

GOAL
Replace this line with what I want, e.g. "a blog with posts, comments and tags".

OUTPUT RULES
- Output ONLY one JSON object. No markdown fences, no explanation, no trailing text.
- Top-level shape: { "collections": [ ... ] }, where each collection has "name", "type" and "schema".
- "type" is "base", "auth" or "view". Use "auth" for accounts (it gets a unique email).
- Canonical field types: ${FIELD_TYPES.join(', ')}.
- Put type-specific settings under each field's "options", e.g. {"values": [...], "maxSelect": 3} for a select, {"collection": "users"} for a relation, {"onCreate": true} for an autodate.
- Do NOT put system fields in "schema" (id, created_at, updated_at, verified, emailVisibility, tokenKey) — the server manages them.
- Every record needs a stable UUID "id". Relation fields hold the target record's id.
- Records are upserted by id. Never emit SQL and never use "deleteMissing".
- Optional: validate the result against https://lazypock.gnuzd.dev/backup.schema.json (reference: https://lazypock.gnuzd.dev/backup).

EXAMPLE (format only — adapt to my goal)
${EXAMPLE}

MY CURRENT COLLECTIONS — extend these and reuse them in relations; do not redeclare system fields
${describeCollections(collections)}`;
}
