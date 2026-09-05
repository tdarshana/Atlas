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

/**
 * The generated frame document for plugin `id`, under the `nonce` the host minted for
 * this frame. Every sandboxed frame has the opaque origin, so the nonce in the URL is
 * what keeps one plugin's frame from loading another plugin's files (SEC-6): the Rust
 * protocol serves `<id>/...` only under that plugin's current nonce.
 */
export function frameUrl(id: string, nonce: string, platform: Platform): string {
	return `${pluginOrigin(platform)}/${encodeURIComponent(id)}/${encodeURIComponent(nonce)}/__frame`;
}

/** One of the plugin's own files, for the rare case the app fetches it directly. */
export function pluginFileUrl(id: string, nonce: string, path: string, platform: Platform): string {
	return `${pluginOrigin(platform)}/${encodeURIComponent(id)}/${encodeURIComponent(nonce)}/${path}`;
}

/** Outside Tauri there is no protocol to mint for; the frame cannot load anyway. */
const DEV_NONCE = 'dev';

/**
 * Asks the host for a fresh nonce for `id`'s frame. Minted in Rust and kept there, so
 * the protocol handler can check it; the value comes back only to go into the URL.
 */
export async function frameNonce(id: string): Promise<string> {
	if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return DEV_NONCE;
	const { invoke } = await import('@tauri-apps/api/core');
	return invoke<string>('plugin_frame_nonce', { id });
}
