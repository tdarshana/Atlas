<script lang="ts">
	// The About card: app and daemon versions, OS, locale, database and folders, with the
	// diagnostics buttons in its head and the Updates group under the list.
	import { errorMessage } from '$lib/errors';
	import { NOTHING } from '$lib/format';
	import { inTauri } from '$lib/shell';
	import { desktop } from '$lib/shell/platform';
	import { status } from '$lib/stores/status.svelte';
	import type { AboutInfo } from '$lib/types';
	import Diagnostics from './Diagnostics.svelte';
	import SettingsCard from './SettingsCard.svelte';
	import Updates from './Updates.svelte';

	let about = $state<AboutInfo | null>(null);
	let aboutError = $state<string | null>(null);
	let diagnostics = $state.raw<Diagnostics>();

	async function loadAbout(): Promise<void> {
		if (!inTauri()) {
			about = null;
			aboutError = null;
			return;
		}
		try {
			about = await desktop('about_info', undefined, () => null);
		} catch (e) {
			aboutError = errorMessage(e);
		}
	}

	/** Re-reads the about info and the diagnostics' UI state; the route calls it on load
	 * and on Reload. */
	export async function refresh(): Promise<void> {
		await loadAbout();
		await diagnostics?.refresh();
	}
</script>

<SettingsCard id="about" title="About">
	{#snippet head()}
		<span class="spacer"></span>
		<Diagnostics {about} bind:this={diagnostics} />
	{/snippet}

	{#if !inTauri()}
		<p class="hint" data-testid="about-browser-hint">Only available in the desktop app.</p>
	{:else if aboutError}
		<p class="bad" role="alert" data-testid="about-error">{aboutError}</p>
	{:else if !about}
		<p class="hint">Loading…</p>
	{/if}
	<dl class="about-list" data-testid="about-info">
		{#if about}
			<dt>App version</dt>
			<dd class="mono">{about.app_version}</dd>
		{/if}
		<dt>Daemon version</dt>
		<dd class="mono">{status.report?.version ?? '…'}</dd>
		{#if about}
			<dt>OS</dt>
			<dd class="mono">{about.os_type} {about.os_version} ({about.arch})</dd>
			<dt>Locale</dt>
			<dd class="mono">{about.locale ?? NOTHING}</dd>
		{/if}
		<dt>Database</dt>
		<dd class="mono">{status.report?.db_path ?? '…'}</dd>
		{#if about}
			<dt>Log folder</dt>
			<dd class="mono">{about.log_dir}</dd>
			<dt>Data folder</dt>
			<dd class="mono">{about.data_dir}</dd>
		{/if}
	</dl>

	<Updates />
</SettingsCard>

<style>
	.spacer {
		flex: 1;
	}

	.mono {
		font-family: var(--font-mono);
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}

	.about-list {
		display: grid;
		grid-template-columns: 140px 1fr;
		row-gap: 6px;
		column-gap: 12px;
	}

	.about-list dt {
		color: var(--text-tertiary);
	}

	.about-list dd {
		margin: 0;
		color: var(--text-primary);
		word-break: break-all;
	}
</style>
