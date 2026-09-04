// Typed wrappers over the plugin Tauri commands in `src-tauri/src/plugins/mod.rs`. Each
// one goes through `desktop` from the shell's platform module, so `bun run dev` in a
// browser gets an empty list and a plain refusal rather than a crash.

import { desktop } from '$lib/shell/platform';
import type { PluginInfo } from './types';

/** What every write command falls back to outside Tauri, where there is no plugin store. */
function noDesktop<T>(): T {
	throw new Error('Plugins need the desktop app.');
}

export function pluginsList(): Promise<PluginInfo[]> {
	return desktop<PluginInfo[]>('plugins_list', undefined, () => []);
}

export function pluginInstallFolder(path: string): Promise<PluginInfo> {
	return desktop<PluginInfo>('plugin_install_folder', { path }, noDesktop);
}

export function pluginInstallGithub(url: string): Promise<PluginInfo> {
	return desktop<PluginInfo>('plugin_install_github', { url }, noDesktop);
}

export function pluginSetEnabled(id: string, enabled: boolean): Promise<PluginInfo> {
	return desktop<PluginInfo>('plugin_set_enabled', { id, enabled }, noDesktop);
}

export function pluginUninstall(id: string): Promise<void> {
	return desktop<void>('plugin_uninstall', { id }, noDesktop);
}

export function pluginReadMain(id: string): Promise<string> {
	return desktop<string>('plugin_read_main', { id }, noDesktop);
}

export function pluginReadFile(id: string, path: string): Promise<string> {
	return desktop<string>('plugin_read_file', { id, path }, noDesktop);
}
