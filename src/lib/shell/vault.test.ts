// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';

import { mirrorKey, vaultHintText, vaultStatus } from './vault';

describe('vaultStatus', () => {
	afterEach(() => {
		vi.restoreAllMocks();
		Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
	});

	it('falls back to "missing" outside Tauri', async () => {
		vi.spyOn(console, 'warn').mockImplementation(() => {});
		expect(await vaultStatus()).toBe('missing');
	});
});

describe('mirrorKey', () => {
	afterEach(() => {
		vi.restoreAllMocks();
		Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
	});

	it('is a no-op outside Tauri', async () => {
		vi.spyOn(console, 'warn').mockImplementation(() => {});
		await expect(mirrorKey('global', 'sk-x')).resolves.toBeUndefined();
	});
});

describe('vaultHintText', () => {
	it('names a missing vault distinctly from a locked one', () => {
		expect(vaultHintText('missing')).toBe('No vault yet, key not mirrored.');
		expect(vaultHintText('locked')).toBe('Vault locked, key not mirrored.');
	});

	it('has nothing to say once the vault is unlocked', () => {
		expect(vaultHintText('unlocked')).toBeNull();
	});
});
