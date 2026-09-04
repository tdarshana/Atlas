// Where a plugin frame's document lives. Tauri exposes a custom scheme as
// `<scheme>://localhost/<path>` on macOS and Linux and remaps it to
// `http://<scheme>.localhost/<path>` on Windows, so the host half differs by platform
// while the path is the same on all three.

import type { Platform } from '$lib/ds';

/** The scheme registered in `src-tauri/src/lib.rs`. */
export const PLUGIN_SCHEME = 'atlas-plugin';

/** The origin a plugin's files are served from on `platform`. */
export function pluginOrigin(platform: Platform): string {
	return platform === 'windows' ? `http://${PLUGIN_SCHEME}.localhost` : `${PLUGIN_SCHEME}://localhost`;
}

/** The generated frame document for plugin `id`. */
export function frameUrl(id: string, platform: Platform): string {
	return `${pluginOrigin(platform)}/${encodeURIComponent(id)}/__frame`;
}

/** One of the plugin's own files, for the rare case the app fetches it directly. */
export function pluginFileUrl(id: string, path: string, platform: Platform): string {
	return `${pluginOrigin(platform)}/${encodeURIComponent(id)}/${path}`;
}
