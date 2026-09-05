<script lang="ts">
	// The Updates group inside the About card: check, download and install, restart.
	import { Button } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { inTauri } from '$lib/shell';
	import { desktop } from '$lib/shell/platform';
	import type { UpdateCheckResult, UpdateProgress } from '$lib/types';
	import { updateProgressPercent } from './update-progress';

	let updateChecking = $state(false);
	let updateResult = $state<UpdateCheckResult | null>(null);
	let updateError = $state<string | null>(null);
	let updateInstalling = $state(false);
	let updateProgress = $state<UpdateProgress | null>(null);

	const updateProgressPct = $derived(updateProgressPercent(updateProgress));

	async function checkForUpdates(): Promise<void> {
		updateChecking = true;
		updateError = null;
		updateResult = null;
		try {
			updateResult = await desktop('update_check', undefined, () => {
				throw new Error('Only available in the desktop app.');
			});
		} catch (e) {
			updateError = errorMessage(e);
		} finally {
			updateChecking = false;
		}
	}

	async function installUpdate(): Promise<void> {
		if (!inTauri()) return;
		updateInstalling = true;
		updateError = null;
		updateProgress = { downloaded: 0, total: null };
		let unlisten: (() => void) | null = null;
		try {
			const { listen } = await import('@tauri-apps/api/event');
			unlisten = await listen<UpdateProgress>('atlas:update-progress', (e) => {
				updateProgress = e.payload;
			});
			const { invoke } = await import('@tauri-apps/api/core');
			// Resolves only on failure: a successful install relaunches the app before
			// this promise would otherwise settle.
			await invoke('update_install');
		} catch (e) {
			updateError = errorMessage(e);
		} finally {
			updateInstalling = false;
			unlisten?.();
		}
	}

	async function restartToUpdate(): Promise<void> {
		await desktop('app_relaunch', undefined, () => undefined);
	}
</script>

<div class="group">
	<span class="group-heading">Updates</span>
	<div class="recorder-row">
		<Button
			variant="ghost"
			size="sm"
			data-testid="update-check"
			disabled={!inTauri() || updateChecking || updateInstalling}
			onclick={checkForUpdates}
		>
			{updateChecking ? 'Checking…' : 'Check for updates'}
		</Button>
		{#if updateError}
			<span class="hint warn" role="status" data-testid="update-error">{updateError}</span>
		{:else if updateResult}
			<span class="hint" role="status" data-testid="update-result">
				{updateResult.available ? `Update available: ${updateResult.version}` : 'Atlas is up to date.'}
			</span>
		{/if}
	</div>
	{#if updateResult?.available}
		<div class="recorder-row">
			<Button
				variant="primary"
				size="sm"
				data-testid="update-install"
				disabled={updateInstalling}
				onclick={installUpdate}
			>
				{updateInstalling ? 'Installing…' : 'Download and install'}
			</Button>
			<Button
				variant="ghost"
				size="sm"
				data-testid="update-restart"
				disabled={updateInstalling}
				onclick={restartToUpdate}
			>
				Restart to update
			</Button>
		</div>
		{#if updateProgress}
			<div class="progress-track" data-testid="update-progress">
				<div class="progress-fill" style={`width: ${updateProgressPct}%`}></div>
			</div>
		{/if}
	{/if}
	{#if updateError === 'updates are not configured'}
		<span class="hint">
			See <a
				href="https://github.com/tdarshana/Atlas#desktop-app"
				target="_blank"
				rel="noreferrer">docs/usage.md, Desktop platform</a
			> for the manual signing key steps.
		</span>
	{/if}
</div>

<style>
	.group {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	/* Same size as the hint it sits under; only the colour says it is a warning. */
	.hint.warn {
		color: var(--danger-text);
	}

	.recorder-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.progress-track {
		height: 4px;
		border-radius: 2px;
		background: var(--bg-base);
		overflow: hidden;
	}

	.progress-fill {
		height: 100%;
		background: var(--accent);
		transition: width 0.2s ease;
	}
</style>
