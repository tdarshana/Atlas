// Theme packs contributed by plugins.
//
// A `themes` contribution names a JSON file inside the plugin's own folder. It is read
// through `plugin_read_file`, which refuses a path that leaves the folder, and then put
// through exactly the same `validateThemePack` an imported pack goes through, so a plugin
// cannot set a token the user could not have set by hand. The pack is renamed to the
// label the Theme select shows, because that label is also the value the setting stores.

import { validateThemePack } from '$lib/shell/theme-pack';
import type { ThemePack } from '$lib/types';
import { pluginReadFile } from './commands';
import type { Contributions, ThemeRef } from './contributions';

/** How a contributed theme is named everywhere the user sees it. */
export function pluginThemeLabel(theme: ThemeRef): string {
	return `${theme.name} (${theme.pluginName})`;
}

/** Reads one plugin file as text. Swapped out in tests. */
export type PluginFileReader = (pluginId: string, path: string) => Promise<string>;

/**
 * Every contributed theme that validates, in contribution order. One that does not is
 * skipped with a console warning and appears nowhere: a plugin must not be able to take
 * the Theme select down with a typo.
 */
export async function loadPluginThemes(
	contribs: Contributions,
	read: PluginFileReader = pluginReadFile
): Promise<ThemePack[]> {
	const packs: ThemePack[] = [];
	for (const theme of contribs.themes) {
		const label = pluginThemeLabel(theme);
		try {
			const text = await read(theme.pluginId, theme.file);
			const raw = JSON.parse(text) as Record<string, unknown>;
			// The manifest, not the file, names the theme in the select; validating the
			// renamed pack is what keeps the label inside the stored pack's own limits.
			packs.push(validateThemePack({ ...raw, name: label }));
		} catch (e) {
			console.warn(
				`Plugin '${theme.pluginId}' theme '${theme.id}' was skipped: ${e instanceof Error ? e.message : String(e)}`
			);
		}
	}
	return packs;
}
