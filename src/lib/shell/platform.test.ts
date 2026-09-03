// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { desktop, inTauri } from './platform';

describe('desktop', () => {
	beforeEach(() => {
		vi.spyOn(console, 'warn').mockImplementation(() => {});
	});

	afterEach(() => {
		vi.restoreAllMocks();
		Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
	});

	it('runs the fallback and warns when there is no Tauri bridge', async () => {
		expect(inTauri()).toBe(false);

		const fallback = vi.fn().mockReturnValue('fallback value');
		const result = await desktop('some_command', undefined, fallback);

		expect(result).toBe('fallback value');
		expect(fallback).toHaveBeenCalledOnce();
		expect(console.warn).toHaveBeenCalledOnce();
	});

	it('warns only once per command name across repeated calls', async () => {
		const fallback = vi.fn().mockReturnValue(undefined);

		await desktop('repeated_command', undefined, fallback);
		await desktop('repeated_command', undefined, fallback);
		await desktop('repeated_command', undefined, fallback);

		expect(fallback).toHaveBeenCalledTimes(3);
		expect(console.warn).toHaveBeenCalledOnce();
	});
});
