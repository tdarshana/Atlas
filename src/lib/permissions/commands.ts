// Typed wrappers over the permission Tauri commands in
// `src-tauri/src/commands/permissions.rs`, plus the System Settings deep link. Each goes
// through `desktop`, so `bun run dev` in a browser gets an honest empty answer rather
// than a crash: outside the desktop app there is no OS to ask.

import { desktop, inTauri } from '$lib/shell/platform';
import type { PermissionId, PermissionStatus } from './system';

/**
 * Every permission's status right now. `roots` are the connected project roots, which the
 * files row probes; the Rust side has no way to know them otherwise, since it never opens
 * the database.
 */
export function permissionsStatus(roots: string[]): Promise<PermissionStatus[]> {
	return desktop<PermissionStatus[]>('permissions_status', { roots }, () => []);
}

/** Asks for one permission, which on macOS means letting the system raise its own prompt. */
export function permissionRequest(id: PermissionId): Promise<PermissionStatus | null> {
	return desktop<PermissionStatus | null>('permission_request', { id }, () => null);
}

/** Opens the System Settings pane a row points at. A no-op in a browser tab, where the
 * `x-apple.systempreferences:` scheme means nothing. */
export async function openSettingsPane(url: string): Promise<void> {
	if (!inTauri()) return;
	const { openUrl } = await import('@tauri-apps/plugin-opener');
	await openUrl(url);
}
