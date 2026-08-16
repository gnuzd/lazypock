// Regenerate studio/src/lib/lazypock.types.ts from the live dev DB schema.
// Dumps _collections + _fields via psql (docker), assembles CollectionSchema[],
// and runs the SDK's generateTypes.
import { execSync } from "node:child_process";
import { writeFileSync } from "node:fs";
import { generateTypes } from "../../../lazypock-ts/dist/index.js";

const psql = (sql) =>
	execSync(
		`docker exec lazypock-postgres psql -U postgres -d lazypock_dev -t -A -F $'\t' -c ${JSON.stringify(sql)}`,
		{ encoding: "utf8" },
	)
		.trim()
		.split("\n")
		.filter(Boolean);

const cols = psql(
	"SELECT id, name, type, system, rules::text, options::text FROM _collections ORDER BY name",
).map((line) => {
	const [id, name, type, system, rules, options] = line.split("\t");
	return { id, name, type, system: system === "t", rules, options };
});

const schemas = cols.map((c) => {
	const fields = psql(
		`SELECT name, type, required, "unique", options::text, indexed, hidden, system, sort_order FROM _fields WHERE collection_id = '${c.id}' ORDER BY sort_order, name`,
	).map((line) => {
		const [name, ftype, required, unique, options, indexed, hidden, system, sort_order] =
			line.split("\t");
		const f = {
			name,
			type: ftype,
			required: required === "t",
			options: options ? JSON.parse(options) : {},
		};
		if (unique === "t") f.unique = true;
		if (indexed === "t") f.indexed = true;
		if (hidden === "t") f.hidden = true;
		if (system === "t") f.system = true;
		if (sort_order !== "0") f.sort_order = parseInt(sort_order, 10);
		return f;
	});
	return {
		id: c.id,
		name: c.name,
		type: c.type,
		system: c.system,
		fields,
		rules: c.rules ? JSON.parse(c.rules) : {},
		options: c.options ? JSON.parse(c.options) : {},
	};
});

const source = generateTypes(schemas, { packageName: "lazypock" });
writeFileSync("src/lib/lazypock.types.ts", source);
console.log(
	`Wrote src/lib/lazypock.types.ts (${schemas.length} collections, ${source.length} bytes)`,
);
