// The client script exists twice: once as a file Rust embeds with `include_str!`, once as
// a string constant the host side can read without a build step. Nothing but this test
// stops the two from drifting apart.

import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { BRIDGE_CLIENT_JS } from './bridge-client';

const RUST_COPY = 'src-tauri/src/plugins/bridge_client.js';

describe('BRIDGE_CLIENT_JS', () => {
	it('is byte-identical to the copy Rust serves', () => {
		expect(BRIDGE_CLIENT_JS).toBe(readFileSync(RUST_COPY, 'utf8'));
	});

	it('defines the client API the host protocol answers', () => {
		for (const name of [
			'window.atlas',
			'atlas:request',
			'atlas:response',
			'atlas:resize',
			'atlas:context',
			'onContext',
			'atlas:tool',
			'atlas:tool-result',
			'onTool'
		]) {
			expect(BRIDGE_CLIENT_JS).toContain(name);
		}
	});
});
