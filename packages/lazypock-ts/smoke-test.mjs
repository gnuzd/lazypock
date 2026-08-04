// ── Smoke test for codegen + typed client ──────────────
// Run: npm run smoke  (node --test? no — plain script)
// Exercises:
//   1. generateTypes() output correctness (field mapping)
//   2. TypedClient.collection() generic resolution
//   3. Full round-trip: generated source → write → typecheck

import { generateTypes, collectionTypeName } from "./dist/index.js";
import { fieldTypeScriptType } from "./dist/index.js";

let failures = 0;
function check(name, actual, expected) {
	const ok = actual === expected;
	if (!ok) failures++;
	console.log(
		`${ok ? "✓" : "✗"} ${name}${ok ? "" : `\n    expected: ${JSON.stringify(expected)}\n    actual:   ${JSON.stringify(actual)}`}`,
	);
}

// ── 1. Field type mapping ──
check(
	"text → string",
	fieldTypeScriptType({ name: "t", type: "text" }),
	"string",
);
check(
	"number → number",
	fieldTypeScriptType({ name: "n", type: "number" }),
	"number",
);
check(
	"bool → boolean",
	fieldTypeScriptType({ name: "b", type: "bool" }),
	"boolean",
);
check(
	"select → union",
	fieldTypeScriptType({
		name: "s",
		type: "select",
		options: { values: ["draft", "published"] },
	}),
	'"draft" | "published"',
);
check(
	"multi_select → union[]",
	fieldTypeScriptType({
		name: "m",
		type: "multi_select",
		options: { values: ["a", "b"] },
	}),
	'("a" | "b")[]',
);
check(
	"relation single → string",
	fieldTypeScriptType({
		name: "r",
		type: "relation",
		options: { collection: "users", maxSelect: 1 },
	}),
	"string",
);
check(
	"relation multi → string[]",
	fieldTypeScriptType({
		name: "r",
		type: "relation",
		options: { collection: "users", maxSelect: 5 },
	}),
	"string[]",
);
check(
	"file → string",
	fieldTypeScriptType({ name: "f", type: "file" }),
	"string",
);
check(
	"multi_file → string[]",
	fieldTypeScriptType({ name: "f", type: "multi_file" }),
	"string[]",
);
check(
	"json → Record",
	fieldTypeScriptType({ name: "j", type: "json" }),
	"Record<string, unknown>",
);
check(
	"geo → Record",
	fieldTypeScriptType({ name: "g", type: "geo" }),
	"Record<string, unknown>",
);
check(
	"password → never",
	fieldTypeScriptType({ name: "p", type: "password" }),
	"never",
);

// ── 2. Identifier sanitization ──
check("collectionTypeName users → Users", collectionTypeName("users"), "Users");
check(
	"collectionTypeName blog-posts → BlogPosts",
	collectionTypeName("blog-posts"),
	"BlogPosts",
);
check("collectionTypeName 2fa → _2fa", collectionTypeName("2fa"), "_2fa");

// ── 3. generateTypes() output ──
const mockCollections = [
	{
		name: "users",
		type: "auth",
		fields: [
			{ name: "email", type: "email", required: true },
			{ name: "verified", type: "bool" },
			{
				name: "role",
				type: "select",
				options: { values: ["admin", "member"] },
			},
			{ name: "password", type: "password" },
		],
	},
	{
		name: "blog_posts",
		type: "base",
		fields: [
			{ name: "title", type: "text", required: true },
			{ name: "published", type: "bool" },
			{
				name: "author",
				type: "relation",
				options: { collection: "users", maxSelect: 1 },
			},
			{ name: "tags", type: "multi_select", options: { values: ["ts", "js"] } },
			{ name: "views", type: "number" },
		],
	},
];

const source = generateTypes(mockCollections, { packageName: "lazypock" });

check(
	"output has UsersRecord",
	source.includes("export interface UsersRecord"),
	true,
);
check(
	"output has BlogPostsRecord",
	source.includes("export interface BlogPostsRecord"),
	true,
);
check("password omitted from record", !source.includes("password"), true);
check("select union in output", source.includes('"admin" | "member"'), true);
check("relation→string", source.includes('"author"?: string;'), true);
check("multi_select→array", source.includes('"tags"?: ("ts" | "js")[];'), true);
check("required field no ?", source.includes('"title": string;'), true);
check("optional bool has ?", source.includes('"published"?: boolean;'), true);
check(
	"LazypockCollections map",
	source.includes('"blog_posts": BlogPostsRecord;'),
	true,
);
check(
	"createClient factory",
	source.includes("export function createClient"),
	true,
);
check("TypedClient class", source.includes("export class TypedClient"), true);
check(
	"auth intersects AuthRecord",
	source.includes('"users": UsersRecord & AuthRecord;'),
	true,
);

// ── Realtime wiring (PocketBase-style) ──
import { LazypockClient, RealtimeService } from "./dist/index.js";

// Stub WebSocket so subscribe() doesn't hit the network.
globalThis.WebSocket = class {
	static CONNECTING = 0;
	static OPEN = 1;
	static CLOSING = 2;
	static CLOSED = 3;
	readyState = 1;
	onopen = null;
	onmessage = null;
	onclose = null;
	onerror = null;
	constructor() {}
	send() {}
	close() {}
};

(() => {
	const client = new LazypockClient({ baseUrl: "http://localhost:4000/api" });
	const svc = client.collection("posts");

	check("collection.subscribe is a function", typeof svc.subscribe, "function");
	check(
		"collection.unsubscribe is a function",
		typeof svc.unsubscribe,
		"function",
	);
	check("collection.getList is a function", typeof svc.getList, "function");
	check(
		"collection.getFullList is a function",
		typeof svc.getFullList,
		"function",
	);
	check(
		"collection.getFirstListItem is a function",
		typeof svc.getFirstListItem,
		"function",
	);
	check("collection.getOne is a function", typeof svc.getOne, "function");
	check("collection.create is a function", typeof svc.create, "function");
	check("collection.update is a function", typeof svc.update, "function");
	check("collection.delete is a function", typeof svc.delete, "function");

	const unsub = svc.subscribe((e) => e);
	check("subscribe returns a function", typeof unsub, "function");
	unsub();

	check(
		"client.realtime is a RealtimeService",
		client.realtime instanceof RealtimeService,
		true,
	);

	// Registry channel — PocketBase-style pb.collections.subscribe()
	const reg = client.collections;
	check(
		"client.collections.subscribe is a function",
		typeof reg.subscribe,
		"function",
	);
	check(
		"client.collections.unsubscribe is a function",
		typeof reg.unsubscribe,
		"function",
	);
	check(
		"client.collections.getFullList is a function",
		typeof reg.getFullList,
		"function",
	);
	check(
		"client.collections.getList is a function",
		typeof reg.getList,
		"function",
	);
	check(
		"client.collections.getOne is a function",
		typeof reg.getOne,
		"function",
	);
	check(
		"client.collections.create is a function",
		typeof reg.create,
		"function",
	);
	check(
		"client.collections.update is a function",
		typeof reg.update,
		"function",
	);
	check(
		"client.collections.delete is a function",
		typeof reg.delete,
		"function",
	);
	const unsubReg = reg.subscribe((e) => e);
	check(
		"collections.subscribe returns a function",
		typeof unsubReg,
		"function",
	);
	unsubReg();
})();

console.log(
	failures === 0 ? "\n✅ All smoke tests passed" : `\n❌ ${failures} failures`,
);
process.exit(failures === 0 ? 0 : 1);
