// Field type registry matching PocketBase's fieldTypes
// Each entry: { icon, label, settings(component), hasInput, hasView }

export const fieldTypes = {
	text: {
		icon: "ri-text",
		label: "Plain text",
	},
	editor: {
		icon: "ri-edit-2-line",
		label: "Rich editor",
	},
	number: {
		icon: "ri-hashtag",
		label: "Number",
	},
	bool: {
		icon: "ri-toggle-line",
		label: "Bool",
	},
	email: {
		icon: "ri-mail-line",
		label: "Email",
	},
	url: {
		icon: "ri-link",
		label: "URL",
	},
	date: {
		icon: "ri-calendar-line",
		label: "Datetime",
	},
	autodate: {
		icon: "ri-calendar-check-line",
		label: "Autodate",
	},
	select: {
		icon: "ri-list-check",
		label: "Select",
	},
	json: {
		icon: "ri-braces-line",
		label: "JSON",
	},
	file: {
		icon: "ri-image-line",
		label: "File",
	},
	relation: {
		icon: "ri-mind-map",
		label: "Relation",
	},
	geoPoint: {
		icon: "ri-map-pin-2-line",
		label: "Geo Point",
	},
	password: {
		icon: "ri-lock-password-line",
		label: "Password",
	},
};

// Types that should appear in the "Add Field" dropdown (exclude password for now, like PB)
export const addableFieldTypes = Object.keys(fieldTypes).filter(
	(t) => t !== "password",
);

/** Generate a unique field name based on type */
export function getUniqueFieldName(fields, type) {
	var baseName = type;
	var result = baseName;
	var counter = 2;

	function hasName(name) {
		if (!fields) return false;
		return fields.some(
			(f) => f.name && f.name.toLowerCase() === name.toLowerCase(),
		);
	}

	while (hasName(result)) {
		result = baseName + counter;
		counter++;
	}

	return result;
}

/** Slugify a field name */
export function slugify(val) {
	return (val || "")
		.toLowerCase()
		.replace(/[^a-z0-9_]+/g, "_")
		.replace(/^_|_$/g, "")
		.replace(/_+/g, "_");
}

/** Create a new field scaffold */
export function createField(type, fields) {
	return {
		id: "",
		name: getUniqueFieldName(fields, type),
		type: type,
		system: false,
		hidden: false,
		required: false,
		presentable: false,
		unique: false,
		indexed: false,
		__focus: true, // auto-focus name input
	};
}
