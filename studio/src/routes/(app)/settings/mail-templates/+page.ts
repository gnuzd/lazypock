import { base } from '$app/paths';
import { redirect } from '@sveltejs/kit';

// The templates editor is the "Mail Templates" tab of the Mail page now.
export function load() {
	throw redirect(307, base + '/settings/mail');
}
