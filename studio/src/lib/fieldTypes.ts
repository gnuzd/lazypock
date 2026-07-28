// Field type registry — matches the Elixir backend field types
// Ported from the old UI's fieldTypes.js

export interface FieldDefinition {
	name: string;
	type: string;
	system: boolean;
	hidden: boolean;
	required: boolean;
	presentable: boolean;
	unique: boolean;
	indexed: boolean;
	[key: string]: unknown;
}

export const fieldTypes: Record<string, { icon: string; label: string }> = {
	text: { icon: 'ri-text', label: 'Plain text' },
	editor: { icon: 'ri-edit-2-line', label: 'Rich editor' },
	number: { icon: 'ri-hashtag', label: 'Number' },
	bool: { icon: 'ri-toggle-line', label: 'Bool' },
	email: { icon: 'ri-mail-line', label: 'Email' },
	url: { icon: 'ri-link', label: 'URL' },
	date: { icon: 'ri-calendar-line', label: 'Datetime' },
	autodate: { icon: 'ri-calendar-check-line', label: 'Autodate' },
	select: { icon: 'ri-list-check', label: 'Select' },
	json: { icon: 'ri-braces-line', label: 'JSON' },
	file: { icon: 'ri-image-line', label: 'File' },
	relation: { icon: 'ri-mind-map', label: 'Relation' },
	geoPoint: { icon: 'ri-map-pin-2-line', label: 'Geo Point' },
	password: { icon: 'ri-lock-password-line', label: 'Password' }
};

// Types that appear in the "Add Field" dropdown
export const addableFieldTypes = Object.keys(fieldTypes).filter((t) => t !== 'password');

/** Generate a unique field name based on type */
export function getUniqueFieldName(fields: FieldDefinition[], type: string): string {
	const baseName = type;
	let result = baseName;
	let counter = 2;

	function hasName(name: string) {
		if (!fields) return false;
		return fields.some((f) => f.name && f.name.toLowerCase() === name.toLowerCase());
	}

	while (hasName(result)) {
		result = baseName + counter;
		counter++;
	}

	return result;
}

/** Slugify a field name */
export function slugify(val: string): string {
	return (val || '')
		.toLowerCase()
		.replace(/[^a-z0-9_]+/g, '_')
		.replace(/^_|_$/g, '')
		.replace(/_+/g, '_');
}

/** Create a new field scaffold */
export function createField(type: string, fields: FieldDefinition[]): FieldDefinition {
	return {
		id: '',
		name: getUniqueFieldName(fields, type),
		type,
		system: false,
		hidden: false,
		required: false,
		presentable: false,
		unique: false,
		indexed: false,
		__focus: true
	};
}
