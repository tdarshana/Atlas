import { redirect } from '@sveltejs/kit';

// Personas are the Agents view since 2026-09-06: one role concept under one name.
export function load() {
	redirect(307, '/agents');
}
