// The project hub's route shape. A dynamic segment has no crawlable entries, so the
// static adapter's index.html fallback serves every tab at runtime.
//
// The project itself is not fetched here. The root layout starts the daemon in `onMount`,
// which is after every load function has run, and inside Tauri the port is only known once
// that call returns; a fetch from here would race it and turn a cold start into an error
// page instead of the shell's own "Starting Atlas…". The layout component loads the
// project once the daemon answers.

import { GLOBAL_ID } from '$lib/stores/project.svelte';
import type { LayoutLoad } from './$types';

export const prerender = false;

export const load: LayoutLoad = ({ params }) => {
	const id = params.id;
	return { id, isGlobal: id === GLOBAL_ID };
};
