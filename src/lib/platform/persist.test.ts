// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';

const desktopMock = vi.fn();

// `persist.ts` only ever talks to Tauri through `desktop`, so mocking it here lets each
// test choose the Tauri or the browser path without touching `window.__TAURI_INTERNALS__`.
vi.mock('$lib/shell/platform', () => ({
	desktop: (command: string, args: Record<string, unknown> | undefined, fallback: () => unknown) =>
		desktopMock(command, args, fallback)
}));

describe('persistSet', () => {
	beforeEach(() => {
		desktopMock.mockReset();
	});

	it('calls ui_state_set through desktop with the key and value', async () => {
		desktopMock.mockImplementation((_command, _args, fallback: () => unknown) =>
			Promise.resolve(fallback())
		);
		const { persistSet } = await import('./persist');

		await persistSet('atlas.theme', 'dark');

		expect(desktopMock).toHaveBeenCalledWith(
			'ui_state_set',
			{ key: 'atlas.theme', value: 'dark' },
			expect.any(Function)
		);
	});

	it('falls back to a no-op when desktop has nothing to invoke through', async () => {
		desktopMock.mockImplementation((_command, _args, fallback: () => unknown) =>
			Promise.resolve(fallback())
		);
		const { persistSet } = await import('./persist');

		await expect(persistSet('atlas.rail', 'expanded')).resolves.toBeUndefined();
	});
});

describe('migrateLocalStorage', () => {
	beforeEach(() => {
		desktopMock.mockReset();
		localStorage.clear();
		vi.resetModules();
	});

	it('copies every atlas.* key into the store once and sets the migrated flag', async () => {
		localStorage.setItem('atlas.theme', 'light');
		localStorage.setItem('atlas.table.tasks', JSON.stringify({ order: ['a'], sort: null }));
		// Not an `atlas.*` key: the migration must leave it alone.
		localStorage.setItem('unrelated', 'value');

		const sets: Array<[string, unknown]> = [];
		desktopMock.mockImplementation(
			(command: string, args: { key: string; value?: unknown } | undefined, fallback: () => unknown) => {
				if (command === 'ui_state_get') return Promise.resolve(null); // nothing migrated yet
				if (command === 'ui_state_set' && args) sets.push([args.key, args.value]);
				return Promise.resolve(fallback());
			}
		);

		const { migrateLocalStorage } = await import('./persist');
		await migrateLocalStorage();

		expect(sets).toContainEqual(['atlas.theme', 'light']);
		expect(sets).toContainEqual(['atlas.table.tasks', { order: ['a'], sort: null }]);
		expect(sets.some(([key]) => key === 'unrelated')).toBe(false);
		expect(sets).toContainEqual(['atlas.persist.migrated', true]);
	});

	it('does nothing once the migrated flag is already set', async () => {
		localStorage.setItem('atlas.theme', 'light');

		desktopMock.mockImplementation((command: string) => {
			if (command === 'ui_state_get') return Promise.resolve(true); // already migrated
			throw new Error(`unexpected call: ${command}`);
		});

		const { migrateLocalStorage } = await import('./persist');
		await migrateLocalStorage();

		expect(desktopMock).toHaveBeenCalledTimes(1);
		expect(desktopMock).toHaveBeenCalledWith('ui_state_get', { key: 'atlas.persist.migrated' }, expect.any(Function));
	});

	it('runs the migration once even when called twice', async () => {
		let getCalls = 0;
		desktopMock.mockImplementation((command: string, _args: unknown, fallback: () => unknown) => {
			if (command === 'ui_state_get') {
				getCalls++;
				return Promise.resolve(null);
			}
			return Promise.resolve(fallback());
		});

		const { migrateLocalStorage } = await import('./persist');
		await Promise.all([migrateLocalStorage(), migrateLocalStorage()]);

		expect(getCalls).toBe(1);
	});
});
