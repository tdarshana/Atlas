import { redirect } from '@sveltejs/kit';

// Board is not a global destination (requirements section 2, "Board is not global -
// it lives under a project"). Until Phase 8 adds `/projects/<id>/board`, send
// visitors to Projects instead.
export function load() {
	redirect(307, '/projects');
}
