import { redirect } from '@sveltejs/kit';

// Practices live on the Skills view since 2026-09-06 (one view for the instructions
// agents follow); old links and the palette land on its Practices tab.
export function load() {
	redirect(307, '/skills?tab=practices');
}
