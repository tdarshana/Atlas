// What a plugin asked for, what it still holds, and what a tick or an untick would leave
// it holding. One place, so the Plugins page and the Permissions view cannot disagree
// about what a plugin may do: `manifest.permissions` is the request, `granted` is the
// answer, and every screen that shows a permission shows the answer.

import type { Permission, PluginInfo } from './types';

/** One permission chip: what the manifest asked for, and whether the user still allows it. */
export interface PermissionChip {
	permission: Permission;
	granted: boolean;
}

/** Everything the manifest asks for, in its own order. Empty when the manifest is missing. */
export function asked(plugin: PluginInfo): Permission[] {
	return plugin.manifest?.permissions ?? [];
}

/** Whether the plugin may do this right now. The grant, never the request. */
export function holds(plugin: PluginInfo, permission: Permission): boolean {
	return (plugin.granted ?? []).includes(permission);
}

/** Whether the plugin asks for anything at all; one that does not has no row to draw. */
export function asksForPermissions(plugin: PluginInfo): boolean {
	return asked(plugin).length > 0;
}

/**
 * The chips a plugin row draws: everything the manifest asked for, each marked with
 * whether it is still granted. Revoking a permission leaves its chip on the row, unticked,
 * so the row keeps saying what the plugin wanted as well as what it has.
 */
export function permissionChips(plugin: PluginInfo): PermissionChip[] {
	return asked(plugin).map((permission) => ({ permission, granted: holds(plugin, permission) }));
}

/**
 * What `plugin_set_permissions` should be given after ticking or unticking `permission`.
 * Always a subset of the manifest and in its order, so a grant for something the manifest
 * no longer asks for cannot be written back.
 */
export function nextGrants(plugin: PluginInfo, permission: Permission, on: boolean): Permission[] {
	return asked(plugin).filter((p) => (p === permission ? on : holds(plugin, p)));
}
