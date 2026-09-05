<script lang="ts">
	// The Command line card: the `atlas` tool ships inside the app, and this installs a
	// `/usr/local/bin/atlas` link to it so the dmg is the only install. Outside the desktop
	// app there is nothing to link, so the card says so and offers nothing.
	import { onMount } from 'svelte';
	import { Button, Input } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { inTauri } from '$lib/shell';
	import { desktop } from '$lib/shell/platform';
	import { push } from '$lib/platform/toasts.svelte';
	import SettingsCard from './SettingsCard.svelte';

	/** What the host answers from `cli_status` and `cli_install`. */
	interface CliStatus {
		bundled: string | null;
		bundled_version: string;
		link: string;
		link_target: string | null;
		linked_to_bundle: boolean;
		shadowed_by: string | null;
	}

	let status = $state<CliStatus | null>(null);
	let busy = $state(false);

	const summary: string = $derived.by((): string => {
		if (!status) return 'Checking…';
		if (!status.bundled) return 'This build carries no atlas binary.';
		if (status.linked_to_bundle) return `Installed: ${status.link} links to this app.`;
		if (status.link_target) return `${status.link} points to ${status.link_target}, not this app.`;
		return `Not installed: nothing at ${status.link}.`;
	});

	const action = $derived(status?.linked_to_bundle ? 'Update link' : 'Install command line tool');

	async function load(): Promise<void> {
		status = await desktop<CliStatus | null>('cli_status', undefined, () => null);
	}

	async function install(): Promise<void> {
		busy = true;
		try {
			status = await desktop<CliStatus>('cli_install', undefined, () => {
				throw new Error('Only available in the desktop app');
			});
			push('success', `atlas is on your PATH at ${status.link}`);
		} catch (e) {
			const message = errorMessage(e);
			if (message !== 'Cancelled.') push('error', message);
		} finally {
			busy = false;
		}
	}

	/** Re-reads the link state; the route calls it on load and on Reload. */
	export async function refresh(): Promise<void> {
		await load();
	}

	onMount(() => {
		void load();
	});
</script>

<SettingsCard id="cli" title="Command line">
	<p class="lead">
		The <code>atlas</code> command (CLI, TUI and the <code>atlas mcp</code> server for
		Claude Code and Codex) ships inside this app. Install it once and every terminal has it.
	</p>
	<div class="pair">
		<Input label="Version" mono readonly value={status?.bundled_version ?? ''} data-testid="settings-cli-version" />
		<Input label="Bundled at" mono readonly value={status?.bundled ?? ''} data-testid="settings-cli-bundled" />
	</div>
	<span class="state" data-testid="settings-cli-state">{summary}</span>
	{#if status?.shadowed_by}
		<span class="warn" data-testid="settings-cli-shadow">
			An older <code>atlas</code> at <code>{status.shadowed_by}</code> comes first on most
			shells. Remove it (<code>rm {status.shadowed_by}</code>) so the linked one runs.
		</span>
	{/if}
	<div class="actions">
		<Button
			size="sm"
			variant="primary"
			disabled={!inTauri() || busy || !status?.bundled}
			title={inTauri() ? undefined : 'Only available in the desktop app'}
			data-testid="settings-cli-install"
			onclick={install}
		>
			{busy ? 'Installing…' : action}
		</Button>
		<span class="hint">Writes a link at <code>{status?.link ?? '/usr/local/bin/atlas'}</code>; macOS may ask for your password.</span>
	</div>
</SettingsCard>

<style>
	.lead {
		margin: 0;
		font-size: 12px;
		color: var(--text-secondary);
	}

	.pair {
		display: grid;
		grid-template-columns: 1fr 2fr;
		gap: 12px;
	}

	.state {
		font-size: 12px;
	}

	.warn {
		font-size: 12px;
		color: var(--warning, var(--text-secondary));
	}

	.actions {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	code {
		font-family: var(--font-mono);
		font-size: 11px;
	}
</style>
