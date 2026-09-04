// Tauri exposes a custom scheme differently per platform, so the frame's URL is the one
// piece of the host that has to know which OS it is on.

import { describe, expect, it } from 'vitest';
import { frameUrl, pluginFileUrl, pluginOrigin } from './frame-url';

describe('frameUrl', () => {
	it('uses the scheme host on macOS and Linux', () => {
		expect(frameUrl('hello-world', 'mac')).toBe('atlas-plugin://localhost/hello-world/__frame');
		expect(frameUrl('hello-world', 'linux')).toBe('atlas-plugin://localhost/hello-world/__frame');
	});

	it('uses the http subdomain Tauri maps the scheme to on Windows', () => {
		expect(frameUrl('hello-world', 'windows')).toBe(
			'http://atlas-plugin.localhost/hello-world/__frame'
		);
	});

	it('escapes an id so it can only ever be one path segment', () => {
		expect(frameUrl('a/b', 'mac')).toBe('atlas-plugin://localhost/a%2Fb/__frame');
	});

	it('names a plugin file under the same origin', () => {
		expect(pluginOrigin('mac')).toBe('atlas-plugin://localhost');
		expect(pluginFileUrl('hello-world', 'theme.css', 'windows')).toBe(
			'http://atlas-plugin.localhost/hello-world/theme.css'
		);
	});
});
