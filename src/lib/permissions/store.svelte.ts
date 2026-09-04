// The system permission statuses, as one store the Permissions view and its side panel
// both read. Two surfaces mount together on arrival, and each asking the OS for itself
// would double the Apple Event round trip and the filesystem probe, so a read already in
// flight is shared rather than repeated.

import { errorMessage } from '$lib/errors';
import { permissionsStatus } from './commands';
import type { PermissionStatus } from './system';

export const systemPermissions = $state({
	statuses: [] as PermissionStatus[],
	/** True once a read has finished at least once, so "nothing granted yet" can be told
	 * apart from "nothing read yet". */
	loaded: false,
	checking: false,
	error: null as string | null
});

/** The read in flight, so two callers in the same tick get one call. */
let inFlight: Promise<void> | null = null;

/**
 * Re-reads every permission. `roots` are the connected project roots the files row probes;
 * pass them after the project list has loaded, or the probe answers about no roots at all.
 */
export function checkPermissions(roots: string[]): Promise<void> {
	if (inFlight) return inFlight;
	systemPermissions.checking = true;
	inFlight = permissionsStatus(roots)
		.then((statuses) => {
			systemPermissions.statuses = statuses;
			systemPermissions.error = null;
		})
		.catch((e) => {
			systemPermissions.error = errorMessage(e);
		})
		.finally(() => {
			systemPermissions.checking = false;
			systemPermissions.loaded = true;
			inFlight = null;
		});
	return inFlight;
}

/** Replaces one row after a `permission_request`, leaving the rest as they were. */
export function putStatus(status: PermissionStatus): void {
	systemPermissions.statuses = [
		...systemPermissions.statuses.filter((s) => s.id !== status.id),
		status
	];
}
