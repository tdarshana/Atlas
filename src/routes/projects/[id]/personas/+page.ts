import { redirect } from '@sveltejs/kit';

// The project's roster is its Agents tab since 2026-09-06.
export function load({ params }: { params: { id: string } }) {
	redirect(307, `/projects/${params.id}/agents`);
}
