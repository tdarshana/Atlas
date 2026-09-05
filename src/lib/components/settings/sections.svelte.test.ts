// @vitest-environment jsdom
// Each Settings section renders on its own against mocked stores (ARCH-10): the route is
// only the list, so a section that cannot stand alone would be a route in disguise.

import { afterEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render } from '@testing-library/svelte';

// The stores these sections read all talk to the daemon through `api()`. Everything a
// section's own load or a store's load asks for answers empty; nothing here asserts on
// the data, only that the card is on screen.
vi.mock('$lib/daemon.svelte', () => {
	const client = {
		getSettings: async () => ({}),
		setSettings: async (partial: unknown) => partial,
		boardStages: async () => ({ stages: [] }),
		listProjects: async () => [],
		listSkills: async () => ({ skills: [], warnings: [] }),
		listMcpServers: async () => ({ servers: [], warnings: [] }),
		mcpStatus: async () => null
	};
	return {
		api: () => client,
		daemon: { port: 7433, ready: true, error: null, logPath: '~/.atlas/atlasd.log' },
		baseUrl: () => 'http://127.0.0.1:7433',
		boot: async () => {}
	};
});

import About from './About.svelte';
import Appearance from './Appearance.svelte';
import BoardStages from './BoardStages.svelte';
import Cli from './Cli.svelte';
import Daemon from './Daemon.svelte';
import Diagnostics from './Diagnostics.svelte';
import McpServers from './McpServers.svelte';
import Notifications from './Notifications.svelte';
import Permissions from './Permissions.svelte';
import Plugins from './Plugins.svelte';
import Shortcuts from './Shortcuts.svelte';
import Skills from './Skills.svelte';
import Updates from './Updates.svelte';
import Vault from './Vault.svelte';

afterEach(cleanup);

describe('settings sections', () => {
	it('Daemon shows the port field', () => {
		const { getByTestId } = render(Daemon);
		expect(getByTestId('settings-port')).not.toBeNull();
		expect(getByTestId('settings-section-daemon')).not.toBeNull();
	});

	it('Cli shows the state line and the install button', () => {
		const { getByTestId } = render(Cli);
		expect(getByTestId('settings-section-cli')).toBeTruthy();
		expect(getByTestId('settings-cli-state').textContent).toContain('Checking');
		expect((getByTestId('settings-cli-install') as HTMLButtonElement).disabled).toBe(true);
	});

	it('BoardStages renders its card', () => {
		const { getByTestId } = render(BoardStages);
		expect(getByTestId('settings-section-board-stages')).not.toBeNull();
	});

	it('Appearance shows the theme select and the import button', () => {
		const { getByTestId } = render(Appearance);
		expect(getByTestId('appearance-theme')).not.toBeNull();
		expect(getByTestId('appearance-import')).not.toBeNull();
	});

	it('McpServers shows the summary line', () => {
		const { getByTestId } = render(McpServers);
		expect(getByTestId('mcp-summary').textContent).toContain('0 servers');
	});

	it('Plugins shows the summary line', () => {
		const { getByTestId } = render(Plugins);
		expect(getByTestId('plugins-settings-summary')).not.toBeNull();
	});

	it('Permissions shows the summary line', () => {
		const { getByTestId } = render(Permissions);
		expect(getByTestId('permissions-settings-summary')).not.toBeNull();
	});

	it('Skills shows the summary line', () => {
		const { getByTestId } = render(Skills);
		expect(getByTestId('skills-settings-summary').textContent).toContain('No skills found yet');
	});

	it('Shortcuts shows the recorder', () => {
		const { getByTestId } = render(Shortcuts);
		expect(getByTestId('shortcut-recorder').textContent).toContain('No shortcut set');
	});

	it('Notifications shows the test button and the three toggles', () => {
		const { getByTestId } = render(Notifications);
		expect(getByTestId('settings-test-notification')).not.toBeNull();
		expect(getByTestId('settings-notify-daemon-errors')).not.toBeNull();
	});

	it('Vault shows the passphrase field and the set button', () => {
		const { getByTestId } = render(Vault);
		expect(getByTestId('vault-passphrase')).not.toBeNull();
		expect(getByTestId('vault-set')).not.toBeNull();
	});

	it('Updates shows the check button', () => {
		const { getByTestId } = render(Updates);
		expect(getByTestId('update-check')).not.toBeNull();
	});

	it('Diagnostics shows both head buttons, disabled without about info', () => {
		const { getByTestId } = render(Diagnostics, { props: { about: null } });
		expect((getByTestId('settings-copy-diagnostics') as HTMLButtonElement).disabled).toBe(true);
		expect(getByTestId('settings-open-log-folder')).not.toBeNull();
	});

	it('About shows the info list and the browser hint outside Tauri', () => {
		const { getByTestId } = render(About);
		expect(getByTestId('about-info')).not.toBeNull();
		expect(getByTestId('about-browser-hint')).not.toBeNull();
	});
});
