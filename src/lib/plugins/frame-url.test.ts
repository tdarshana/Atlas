// Tauri exposes a custom scheme differently per platform, so the frame's URL is the one
// piece of the host that has to know which OS it is on.

import { describe, expect, it } from 'vitest';
import { frameUrl, pluginFileUrl, pluginOrigin } from './frame-url';

describe('frameUrl', () => {
	it('uses the scheme host on macOS and Linux', () => {
		expect(frameUrl('hello-world', 'n1', 'mac')).toBe('atlas-plugin://localhost/hello-world/n1/__frame');
		expect(frameUrl('hello-world', 'n1', 'linux')).toBe('atlas-plugin://localhost/hello-world/n1/__frame');
	});

	it('uses the http subdomain Tauri maps the scheme to on Windows', () => {
		expect(frameUrl('hello-world', 'n1', 'windows')).toBe(
			'http://atlas-plugin.localhost/hello-world/n1/__frame'
		);
	});

	// SEC-6: the nonce the host minted sits between the id and the document, so the
	// protocol can refuse a frame that names another plugin's files.
	it('puts the frame nonce between the id and the document', () => {
		const url = frameUrl('hello-world', '0123abcd', 'mac');
		expect(url).toBe('atlas-plugin://localhost/hello-world/0123abcd/__frame');
		expect(new URL(url).pathname.split('/')).toEqual(['', 'hello-world', '0123abcd', '__frame']);
	});

	it('escapes an id and a nonce so each can only ever be one path segment', () => {
		expect(frameUrl('a/b', 'n1', 'mac')).toBe('atlas-plugin://localhost/a%2Fb/n1/__frame');
		expect(frameUrl('a', 'x/y', 'mac')).toBe('atlas-plugin://localhost/a/x%2Fy/__frame');
	});

	it('names a plugin file under the same origin and nonce', () => {
		expect(pluginOrigin('mac')).toBe('atlas-plugin://localhost');
		expect(pluginFileUrl('hello-world', 'n1', 'theme.css', 'windows')).toBe(
			'http://atlas-plugin.localhost/hello-world/n1/theme.css'
		);
	});
});
