<script lang="ts">
	// The Daemon card: port and embedding model, read only because the daemon sets them at
	// startup, and the launch-at-login toggle.
	import { Checkbox, Input } from '$lib/ds';
	import { api, daemon } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { inTauri } from '$lib/shell';
	import { desktop } from '$lib/shell/platform';
	import { settingString } from '$lib/stores/settings.svelte';
	import { push } from '$lib/platform/toasts.svelte';
	import SettingsCard from './SettingsCard.svelte';

	const port = $derived(settingString('daemon.port', String(daemon.port)));
	const embeddingModel = $derived(settingString('embedding.model', 'not set') || 'not set');

	// The checkbox reflects the actual launch-agent state (`autostart_get`), not the
	// mirrored `ui.autostart` setting: another client's read of that mirror is a
	// convenience, not the source of truth this OS-level toggle has to agree with.
	let autostart = $state(false);
	let autostartBusy = $state(false);

	async function loadAutostart(): Promise<void> {
		autostart = await desktop('autostart_get', undefined, () => false);
	}

	async function toggleAutostart(next: boolean): Promise<void> {
		autostartBusy = true;
		try {
			await desktop('autostart_set', { enabled: next }, () => undefined);
			autostart = next;
			await api().setSettings({ 'ui.autostart': next });
		} catch (e) {
			push('error', errorMessage(e));
			await loadAutostart();
		} finally {
			autostartBusy = false;
		}
	}

	/** Re-reads the launch-agent state; the route calls it on load and on Reload. */
	export async function refresh(): Promise<void> {
		await loadAutostart();
	}
</script>

<SettingsCard id="daemon" title="Daemon">
	<div class="pair">
		<Input label="Port" mono readonly value={port} data-testid="settings-port" />
		<Input
			label="Embedding model"
			mono
			readonly
			value={embeddingModel}
			data-testid="settings-embedding"
		/>
	</div>
	<span class="hint">Both are set when the daemon starts and are shown here for reference.</span>
	<span class="hint">The daemon keeps running after Atlas closes, for the TUI and <code>atlas mcp</code>; the menu bar item shows it and can stop it.</span>
	<Checkbox
		label="Start Atlas at login"
		checked={autostart}
		disabled={!inTauri() || autostartBusy}
		title={inTauri() ? undefined : 'Only available in the desktop app'}
		data-testid="settings-autostart"
		onchange={() => toggleAutostart(!autostart)}
		onreset={autostart && inTauri() && !autostartBusy ? () => toggleAutostart(false) : undefined}
	/>
</SettingsCard>

<style>
	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
