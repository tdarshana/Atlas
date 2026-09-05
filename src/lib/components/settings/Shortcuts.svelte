<script lang="ts">
	// The Shortcuts card: the global shortcut recorder and the in-app combos the shell binds.
	import { Button, KeyHint } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { inTauri } from '$lib/shell';
	import { desktop } from '$lib/shell/platform';
	import { VIEWS } from '$lib/shell/views';
	import { settingString } from '$lib/stores/settings.svelte';
	import { push } from '$lib/platform/toasts.svelte';
	import {
		acceleratorToKeyHintCombo,
		blurRecording,
		cancelRecording,
		captureKey,
		comboToAccelerator,
		INITIAL_RECORDER_STATE,
		startRecording as startRecordingCombo,
		type RecorderState
	} from './shortcut-recorder';
	import SettingsCard from './SettingsCard.svelte';

	let recorder = $state<RecorderState>(INITIAL_RECORDER_STATE);
	let shortcutError = $state<string | null>(null);
	let applyingShortcut = $state(false);

	const capturedAccelerator = $derived(comboToAccelerator(recorder.combo));
	const storedShortcut = $derived(settingString('ui.global_shortcut'));
	/** What the recorder shows: the combo being captured, or the stored shortcut when
	 * nothing is being recorded right now. */
	const recorderCombo = $derived(
		capturedAccelerator
			? acceleratorToKeyHintCombo(capturedAccelerator)
			: storedShortcut
				? acceleratorToKeyHintCombo(storedShortcut)
				: ''
	);

	function startRecording(): void {
		recorder = startRecordingCombo();
		shortcutError = null;
	}

	/**
	 * Losing focus must not lose the captured combo: the Apply button sits right next
	 * to the recorder, so a plain click on it fires blur before click in a
	 * click-focuses-buttons engine, and clearing the combo here would make Apply
	 * unclickable on every attempt. `blurRecording` only stops recording.
	 */
	function onRecorderBlur(): void {
		recorder = blurRecording(recorder);
	}

	function onRecorderKeydown(e: KeyboardEvent): void {
		if (!recorder.recording) return;
		e.preventDefault();
		if (e.key === 'Escape') {
			recorder = cancelRecording();
			return;
		}
		recorder = captureKey(recorder, e);
	}

	async function applyShortcut(): Promise<void> {
		const accelerator = capturedAccelerator;
		if (!accelerator) return;
		applyingShortcut = true;
		shortcutError = null;
		try {
			await desktop('shortcut_set', { accelerator }, () => undefined);
			await api().setSettings({ 'ui.global_shortcut': accelerator });
			recorder = cancelRecording();
			push('success', 'Shortcut applied');
		} catch (e) {
			shortcutError = errorMessage(e);
		} finally {
			applyingShortcut = false;
		}
	}

	/** Mod+K, Mod+J and Mod+B are bound by the shell directly, not per-view; the rest
	 * come straight from the rail's own view list so this can never list a combo the
	 * shell does not actually bind. */
	const inAppShortcuts = [
		{ combo: 'Mod+K', label: 'Command palette' },
		{ combo: 'Mod+J', label: 'Toggle side panel' },
		{ combo: 'Mod+B', label: 'Toggle rail' },
		...VIEWS.map((v) => ({ combo: v.combo, label: v.label }))
	];
</script>

<SettingsCard id="shortcuts" title="Shortcuts">
	<div class="group">
		<span class="group-heading">Global shortcut</span>
		<span class="hint">
			Works from anywhere, even when Atlas is not the focused app: shows the window
			and opens the command palette.
		</span>
		<div class="recorder-row">
			<button
				type="button"
				class="recorder"
				data-testid="shortcut-recorder"
				disabled={!inTauri()}
				title={inTauri() ? 'Click, then press a key combination' : 'Only available in the desktop app'}
				onclick={startRecording}
				onkeydown={onRecorderKeydown}
				onblur={onRecorderBlur}
			>
				{#if recorder.recording && !capturedAccelerator}
					Press a key combination…
				{:else if recorderCombo}
					<KeyHint combo={recorderCombo} />
				{:else}
					No shortcut set
				{/if}
			</button>
			<Button
				variant="primary"
				size="sm"
				data-testid="shortcut-apply"
				disabled={!capturedAccelerator || applyingShortcut}
				onclick={applyShortcut}
			>
				{applyingShortcut ? 'Applying…' : 'Apply'}
			</Button>
		</div>
		{#if shortcutError}
			<p class="bad" role="alert" data-testid="shortcut-error">{shortcutError}</p>
		{/if}
	</div>

	<div class="group">
		<span class="group-heading">In-app shortcuts</span>
		<div class="shortcut-list">
			{#each inAppShortcuts as s (s.combo)}
				<div class="shortcut-row">
					<span>{s.label}</span>
					<KeyHint combo={s.combo} plain />
				</div>
			{/each}
		</div>
	</div>
</SettingsCard>

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

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}

	.recorder-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.recorder {
		flex: 1;
		height: 28px;
		padding: 0 10px;
		display: flex;
		align-items: center;
		border: 1px solid var(--border-default);
		border-radius: 3px;
		background: var(--bg-base);
		color: var(--text-secondary);
		font-size: 12px;
		text-align: left;
		cursor: pointer;
	}

	.recorder:disabled {
		cursor: default;
		opacity: 0.5;
	}

	.recorder:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: -1px;
	}

	.shortcut-list {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.shortcut-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		height: 22px;
		color: var(--text-secondary);
	}
</style>
