<script lang="ts">
	// Settings: the seven keys the daemon stores. Port and embedding model are read
	// only here (they are daemon startup facts); the extraction block is editable.
	// Save sends only the keys that changed, and the API key only when one was typed,
	// because the server hands back "***" for a stored key and treats it as "leave it".
	import { onMount } from 'svelte';
	import {
		MASKED,
		loadSettings,
		saveSettings,
		settingBool,
		settingNumber,
		settingString,
		settings,
		DEFAULT_MIN_CONFIDENCE
	} from '$lib/stores/settings.svelte';
	import { daemon } from '$lib/daemon.svelte';
	import type { Settings } from '$lib/types';
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

	/** True once the daemon holds a key, which is all `"***"` tells us. */
	const keyStored = $derived(settings.values['extraction.api_key'] === MASKED);
	const port = $derived(settingString('daemon.port', String(daemon.port)));
	const embeddingModel = $derived(settingString('embedding.model', '—') || '—');

	/** Copies the server's values into the draft, discarding any unsaved edits. */
	function syncDraft() {
		enabled = settingBool('extraction.enabled');
		baseUrl = settingString('extraction.base_url');
		model = settingString('extraction.model');
		threshold = settingNumber('extraction.auto_accept_min_confidence', DEFAULT_MIN_CONFIDENCE);
		apiKey = '';
	}

	/** Only the keys the user actually changed; floats compare with a tolerance. */
	function changes(): Settings {
		const out: Settings = {};
		if (enabled !== settingBool('extraction.enabled')) out['extraction.enabled'] = enabled;
		if (baseUrl !== settingString('extraction.base_url')) out['extraction.base_url'] = baseUrl;
		if (model !== settingString('extraction.model')) out['extraction.model'] = model;
		const stored = settingNumber('extraction.auto_accept_min_confidence', DEFAULT_MIN_CONFIDENCE);
		if (Math.abs(threshold - stored) > 1e-9) {
			out['extraction.auto_accept_min_confidence'] = threshold;
		}
		// A blank box means "leave the stored key alone", so never send "" or "***".
		if (apiKey !== '') out['extraction.api_key'] = apiKey;
		return out;
	}

	async function save() {
		const partial = changes();
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
			saveError = e instanceof Error ? e.message : String(e);
			push('error', saveError);
		} finally {
			saving = false;
		}
	}

	async function reload() {
		await loadSettings();
		syncDraft();
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
					disabled
					title="available after extraction is set up"
				>
					Test connection
				</Button>
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
