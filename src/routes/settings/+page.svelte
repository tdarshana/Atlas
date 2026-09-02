<script lang="ts">
	// Settings: the seven keys the daemon stores. Port and embedding model are read
	// only here (they are daemon startup facts); the extraction block is editable.
	// Save sends only the keys that changed, and the API key only when one was typed,
	// because the server hands back "***" for a stored key and treats it as "leave it".
	import { onMount } from 'svelte';
	import {
		DEFAULT_MIN_CONFIDENCE,
		MASKED,
		changedSettings,
		draftFromSettings,
		loadSettings,
		saveSettings,
		settingString,
		settings
	} from '$lib/stores/settings.svelte';
	import { api, daemon } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import type { ExtractionTestResult } from '$lib/types';
	import Button from '$lib/ui/Button.svelte';
	import Card from '$lib/ui/Card.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Input from '$lib/ui/Input.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	/** Draft values. The API key starts blank on every load: blank means unchanged. */
	let enabled = $state(false);
	let baseUrl = $state('');
	let apiKey = $state('');
	let model = $state('');
	let threshold = $state(DEFAULT_MIN_CONFIDENCE);

	let saving = $state(false);
	let saveError = $state<string | null>(null);

	let testing = $state(false);
	let testResult = $state<ExtractionTestResult | null>(null);

	/** The button needs extraction on and both endpoint fields filled to mean anything. */
	const testDisabled = $derived(
		!enabled || baseUrl.trim() === '' || model.trim() === '' || testing
	);

	/** True once the daemon holds a key, which is all `"***"` tells us. */
	const keyStored = $derived(settings.values['extraction.api_key'] === MASKED);

	/**
	 * A key is entered against one endpoint, so the daemon drops it when the base URL
	 * moves without a new one: nothing local can point Atlas somewhere else and have the
	 * old key follow it there. Warn before saving, not after, since the key has to be
	 * typed again either way. A trailing slash is not a move; neither is it to the daemon.
	 */
	const trimTrailingSlash = (url: string) => url.trim().replace(/\/+$/, '');
	const keyWillBeCleared = $derived(
		keyStored &&
			apiKey === '' &&
			trimTrailingSlash(baseUrl) !== trimTrailingSlash(settingString('extraction.base_url'))
	);
	const port = $derived(settingString('daemon.port', String(daemon.port)));
	const embeddingModel = $derived(settingString('embedding.model', 'not set') || 'not set');

	/** Copies the server's values into the draft, discarding any unsaved edits. */
	function syncDraft() {
		const d = draftFromSettings();
		enabled = d.enabled;
		baseUrl = d.baseUrl;
		apiKey = d.apiKey;
		model = d.model;
		threshold = d.threshold;
	}

	async function save() {
		// The diff lives in the store so it can be unit tested; a blank API key box
		// means "leave the stored key alone" and sends nothing for that key.
		const partial = changedSettings({ enabled, baseUrl, apiKey, model, threshold });
		if (Object.keys(partial).length === 0) {
			push('info', 'No changes to save');
			return;
		}
		saving = true;
		saveError = null;
		try {
			await saveSettings(partial);
			syncDraft();
			push('success', 'Settings saved');
		} catch (e) {
			// The daemon's `error` string is the whole explanation; show it verbatim.
			saveError = errorMessage(e);
			push('error', saveError);
		} finally {
			saving = false;
		}
	}

	async function reload() {
		await loadSettings();
		syncDraft();
	}

	/** Saves the form first when it is dirty, then calls `/extraction/test`. */
	async function testConnection() {
		if (Object.keys(changedSettings({ enabled, baseUrl, apiKey, model, threshold })).length > 0) {
			await save();
			if (saveError) return;
		}
		testing = true;
		testResult = null;
		try {
			testResult = await api().testExtraction();
			if (testResult.ok) {
				push('success', `Connected. Reply: ${testResult.reply}`);
			} else {
				push('error', testResult.error ?? 'Connection failed');
			}
		} catch (e) {
			const message = errorMessage(e);
			testResult = { ok: false, error: message };
			push('error', message);
		} finally {
			testing = false;
		}
	}

	onMount(() => {
		void reload();
	});
</script>

<h1>Settings</h1>

{#if settings.error && !settings.loaded}
	<ErrorState message={settings.error} logPath={settings.errorLogPath ?? undefined}>
		<Button variant="primary" onclick={reload}>Retry</Button>
	</ErrorState>
{:else}
	<form
		data-testid="settings-form"
		onsubmit={(e: SubmitEvent) => {
			e.preventDefault();
			void save();
		}}
	>
		<Card title="Daemon">
			<div class="row">
				<div class="field">
					<span>Port</span>
					<p class="ro" data-testid="settings-port">{port}</p>
				</div>
				<div class="field">
					<span>Embedding model</span>
					<p class="ro" data-testid="settings-embedding">{embeddingModel}</p>
				</div>
			</div>
			<p class="hint">Both are set when the daemon starts and are shown here for reference.</p>
		</Card>

		<Card title="Extraction">
			{#snippet actions()}
				<Button
					data-testid="settings-test"
					disabled={testDisabled}
					title={testDisabled
						? 'Enable extraction and set a base URL and model first'
						: 'Send a test request to the configured endpoint'}
					onclick={testConnection}
				>
					{testing ? 'Testing…' : 'Test connection'}
				</Button>
				{#if testResult}
					<span
						class="test-result"
						class:bad={!testResult.ok}
						role="status"
						data-testid="extraction-test-result"
					>
						{testResult.ok ? `Connected. Reply: ${testResult.reply}` : testResult.error}
					</span>
				{/if}
			{/snippet}

			<div class="form">
				<label class="toggle">
					<input type="checkbox" bind:checked={enabled} data-testid="settings-enabled" />
					<span>Enable extraction</span>
				</label>

				<label class="field">
					<span>Base URL</span>
					<Input
						bind:value={baseUrl}
						data-testid="settings-base-url"
						placeholder="https://api.deepseek.com"
					/>
				</label>

				<label class="field">
					<span>API key</span>
					<Input
						type="password"
						bind:value={apiKey}
						data-testid="settings-api-key"
						autocomplete="off"
						placeholder={keyStored ? 'unchanged' : 'sk-…'}
					/>
					<span class="hint">
						{keyStored
							? 'A key is stored. Leave this blank to keep it.'
							: 'No key stored yet.'}
					</span>
					{#if keyWillBeCleared}
						<span class="hint warn" role="status" data-testid="settings-key-cleared-note">
							Changing the base URL clears the stored key; enter it again.
						</span>
					{/if}
				</label>

				<label class="field">
					<span>Model</span>
					<Input bind:value={model} data-testid="settings-model" placeholder="deepseek-chat" />
				</label>

				<label class="field">
					<span>Auto-accept threshold</span>
					<span class="slider">
						<input
							type="range"
							min="0"
							max="1"
							step="0.05"
							bind:value={threshold}
							data-testid="settings-threshold"
						/>
						<output data-testid="settings-threshold-value">{threshold.toFixed(2)}</output>
					</span>
					<span class="hint">
						Review's "Accept all above threshold" uses this value.
					</span>
				</label>

				<p class="hint" data-testid="extraction-key-note">
					The API key is stored in the local Atlas database, sent only to the base URL
					above, and never logged.
				</p>

				{#if saveError}
					<p class="bad" role="alert" data-testid="settings-error">{saveError}</p>
				{/if}
			</div>
		</Card>

		<div class="foot">
			<Button variant="primary" type="submit" data-testid="settings-save" disabled={saving}>
				{saving ? 'Saving…' : 'Save'}
			</Button>
			<Button onclick={reload} disabled={saving || settings.loading}>Reload</Button>
		</div>
	</form>
{/if}

<style>
	form {
		display: flex;
		flex-direction: column;
		gap: var(--space-4);
	}

	.form {
		display: flex;
		flex-direction: column;
		gap: var(--space-4);
	}

	.row {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-4);
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}

	/* Only fields laid out side by side share the row's width. Inside `.form`, which
	   stacks its children, the same basis is read as a height and stretches every
	   field to 240px. */
	.row > .field {
		flex: 1 1 240px;
	}

	.field > span:first-child {
		font-size: 13px;
		color: var(--muted);
	}

	.ro {
		margin: 0;
		padding: 7px 10px;
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		color: var(--fg);
		font-family: var(--font-mono, monospace);
	}

	.toggle {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}

	.slider {
		display: flex;
		align-items: center;
		gap: var(--space-3);
	}

	.slider input {
		flex: 1;
	}

	.slider output {
		min-width: 3.5ch;
		color: var(--muted);
		font-variant-numeric: tabular-nums;
	}

	.hint {
		margin: 0;
		color: var(--muted);
		font-size: 12px;
	}

	/* Same size as the hint it sits under; only the colour says it is a warning. */
	.hint.warn {
		color: var(--danger);
	}

	.test-result {
		font-size: 13px;
		color: var(--accent);
	}

	.test-result.bad {
		color: var(--danger);
	}

	.bad {
		margin: 0;
		color: var(--danger);
		font-size: 13px;
	}

	.foot {
		display: flex;
		gap: var(--space-2);
	}
</style>
