// The installed plugins, as one store the Plugins page, its side panel and the Settings
// summary all read. `available` is false outside Tauri, where there is no plugin store to
// read at all, so a screen can say so instead of showing an empty table as if none were
// installed.

import { errorMessage } from '$lib/errors';
import { inTauri } from '$lib/shell/platform';
import {
	pluginInstallFolder,
	pluginInstallGithub,
	pluginSetEnabled,
	pluginUninstall,
	pluginsList
} from './commands';
import type { Bridge } from './bridge';
import { collectContributions, type Contributions } from './contributions';
import type { PluginInfo } from './types';

export const plugins = $state({
	items: [] as PluginInfo[],
	loading: false,
	/** True once `loadPlugins` has finished at least once, so an empty list can be told
	 * apart from a list that has not been fetched yet. */
	loaded: false,
	error: null as string | null,
	available: inTauri()
});

/** Everything the enabled, compatible plugins contribute right now. */
export function contributions(): Contributions {
	return collectContributions(plugins.items);
}

export function enabledCount(): number {
	return plugins.items.filter((p) => p.enabled).length;
}

/** One installed plugin by id, for a surface holding only a contribution ref. */
export function pluginById(id: string): PluginInfo | undefined {
	return plugins.items.find((p) => p.id === id);
}

// -- mounted frames ---------------------------------------------------------------------
//
// A palette command has to reach the plugin's frames, wherever they happen to be on
// screen. Every `PluginFrame` puts its bridge in here while it is mounted and takes it
// out again when it is torn down, so a command dispatch is a lookup rather than a hunt
// through the DOM. Plain module state, not `$state`: nothing renders from it.

const mountedFrames = new Map<string, Set<Bridge>>();

export function registerFrame(pluginId: string, bridge: Bridge): void {
	const set = mountedFrames.get(pluginId) ?? new Set<Bridge>();
	set.add(bridge);
	mountedFrames.set(pluginId, set);
}

export function unregisterFrame(pluginId: string, bridge: Bridge): void {
	const set = mountedFrames.get(pluginId);
	if (!set) return;
	set.delete(bridge);
	if (set.size === 0) mountedFrames.delete(pluginId);
}

/** Sends `commandId` to every mounted frame of `pluginId`, and answers how many got it. */
export function dispatchCommand(pluginId: string, commandId: string): number {
	const set = mountedFrames.get(pluginId);
	if (!set) return 0;
	for (const bridge of set) bridge.sendCommand(commandId);
	return set.size;
}

export async function loadPlugins(): Promise<void> {
	plugins.available = inTauri();
	if (!plugins.available) {
		plugins.items = [];
		plugins.loaded = true;
		return;
	}
	plugins.loading = true;
	try {
		// A command that answers with nothing at all leaves the list empty rather than
		// undefined, so every reader can keep treating `items` as an array.
		const listed = await pluginsList();
		plugins.items = Array.isArray(listed) ? listed : [];
		plugins.error = null;
	} catch (e) {
		plugins.error = errorMessage(e);
	} finally {
		plugins.loading = false;
		plugins.loaded = true;
	}
}

/** Puts an install or a toggle's returned info back into the list, in id order. */
function upsert(info: PluginInfo): void {
	const rest = plugins.items.filter((p) => p.id !== info.id);
	plugins.items = [...rest, info].sort((a, b) => a.id.localeCompare(b.id));
}

export async function installFolder(path: string): Promise<PluginInfo> {
	const info = await pluginInstallFolder(path);
	upsert(info);
	return info;
}

export async function installGithub(url: string): Promise<PluginInfo> {
	const info = await pluginInstallGithub(url);
	upsert(info);
	return info;
}

export async function setEnabled(id: string, on: boolean): Promise<PluginInfo> {
	const info = await pluginSetEnabled(id, on);
	upsert(info);
	return info;
}

export async function uninstall(id: string): Promise<void> {
	await pluginUninstall(id);
	plugins.items = plugins.items.filter((p) => p.id !== id);
}
