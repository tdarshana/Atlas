<script lang="ts">
	// Settings (frame 10): the seven keys the daemon stores, the global board stages, and
	// how agents reach the MCP server. Port and embedding model are read only because the
	// daemon sets them at startup. Save sends only the keys that changed, and the API key
	// only when one was typed: the server hands back "***" for a stored key and reads a
	// missing key as "leave it alone".
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { Badge, Button, Checkbox, Input } from '$lib/ds';
	import { api, daemon } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { setStatusItems } from '$lib/shell';
	import { scrollToSection } from '$lib/components/settings-sections';
	import StageEditor from '$lib/components/StageEditor.svelte';
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
	import type { ExtractionTestResult, Stage } from '$lib/types';
	import ErrorState from '$lib/ui/ErrorState.svelte';
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

	/** The global stage list, which every project without an override follows. */
	let stages = $state<Stage[]>([]);
	let stagesError = $state<string | null>(null);

	let copied = $state<string | null>(null);

	/** The button needs extraction on and both endpoint fields filled to mean anything. */
	const testDisabled = $derived(!enabled || baseUrl.trim() === '' || model.trim() === '' || testing);

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
	const online = $derived(!daemon.error);
	const httpEndpoint = $derived(`http://127.0.0.1:${port}/mcp`);

	const CLAUDE_SNIPPET = 'claude mcp add --scope user atlas -- atlas mcp';
	const CODEX_SNIPPET = `# ~/.codex/config.toml
[mcp_servers.atlas]
command = "atlas"
args = ["mcp"]`;

	/** Copies a snippet and names which one was copied, so the feedback is on the button. */
	async function copy(label: string, text: string): Promise<void> {
		try {
			await navigator.clipboard.writeText(text);
			copied = label;
		} catch (e) {
			push('error', `Could not copy: ${errorMessage(e)}`);
		}
	}

	/** Copies the server's values into the draft, discarding any unsaved edits. */
	function syncDraft(): void {
		const d = draftFromSettings();
		enabled = d.enabled;
		baseUrl = d.baseUrl;
		apiKey = d.apiKey;
		model = d.model;
		threshold = d.threshold;
	}

	async function save(): Promise<void> {
		// The diff lives in the store so it can be unit tested; a blank API key box means
		// "leave the stored key alone" and sends nothing for that key.
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

	async function reload(): Promise<void> {
		await loadSettings();
		syncDraft();
		await loadStages();
	}

	async function loadStages(): Promise<void> {
		try {
			stages = (await api().boardStages()).stages;
			stagesError = null;
		} catch (e) {
			stages = [];
			stagesError = errorMessage(e);
		}
	}

	async function saveStages(next: Stage[], renames: Record<string, string>): Promise<void> {
		try {
			stages = await api().setBoardStages(next, renames);
			stagesError = null;
			push('success', 'Board stages saved');
		} catch (e) {
			stagesError = errorMessage(e);
			push('error', stagesError);
		}
	}

	/** Saves the form first when it is dirty, then calls `/extraction/test`. */
	async function testConnection(): Promise<void> {
		if (Object.keys(changedSettings({ enabled, baseUrl, apiKey, model, threshold })).length > 0) {
			await save();
			if (saveError) return;
		}
		testing = true;
		testResult = null;
		try {
			testResult = await api().testExtraction();
			if (testResult.ok) push('success', `Connected. Reply: ${testResult.reply}`);
			else push('error', testResult.error ?? 'Connection failed');
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

	// `/settings#<section>` (the side panel's Sections rows) scrolls to that card once its
	// content has loaded. It reads the hash rather than running once, so a second row
	// picked while this page is already open scrolls too.
	$effect(() => {
		scrollToSection(page.url.hash, settings.loaded, (id) => document.getElementById(id));
	});

	$effect(() => {
		setStatusItems({ right: [{ text: 'settings' }] });
	});
</script>

<div class="title-row"><span class="title">Settings</span></div>

{#if settings.error && !settings.loaded}
	<ErrorState message={settings.error} logPath={settings.errorLogPath ?? undefined}>
		<Button variant="primary" onclick={reload}>Retry</Button>
	</ErrorState>
{:else}
	<div class="pane" data-testid="settings-form">
		<section class="card" id="daemon">
			<div class="card-head"><span class="card-title">Daemon</span></div>
			<div class="card-body">
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
			</div>
		</section>

		<section class="card" id="extraction">
			<div class="card-head">
				<span class="card-title">Extraction</span>
				<span class="spacer"></span>
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
				<Button
					variant="ghost"
					size="sm"
					data-testid="settings-test"
					disabled={testDisabled}
					title={testDisabled
						? 'Enable extraction and set a base URL and model first'
						: 'Send a test request to the configured endpoint'}
					onclick={testConnection}
				>
					{testing ? 'Testing…' : 'Test connection'}
				</Button>
			</div>

			<div class="card-body">
				<Checkbox label="Enable extraction" bind:checked={enabled} data-testid="settings-enabled" />

				<div class="pair">
					<Input
						label="Base URL"
						mono
						bind:value={baseUrl}
						data-testid="settings-base-url"
						placeholder="https://api.deepseek.com"
					/>
					<Input
						label="Model"
						mono
						bind:value={model}
						data-testid="settings-model"
						placeholder="deepseek-chat"
					/>
				</div>

				<Input
					label="API key"
					type="password"
					mono
					bind:value={apiKey}
					autocomplete="off"
					data-testid="settings-api-key"
					placeholder={keyStored ? 'unchanged' : 'sk-…'}
					hint={keyStored ? 'A key is stored. Leave this blank to keep it.' : 'No key stored yet.'}
				/>
				{#if keyWillBeCleared}
					<span class="hint warn" role="status" data-testid="settings-key-cleared-note">
						Changing the base URL clears the stored key; enter it again.
					</span>
				{/if}

				<div class="field">
					<span class="label">Auto-accept threshold</span>
					<div class="slider">
						<input
							type="range"
							min="0"
							max="1"
							step="0.05"
							bind:value={threshold}
							aria-label="Auto-accept threshold"
							data-testid="settings-threshold"
						/>
						<output class="mono" data-testid="settings-threshold-value">
							{threshold.toFixed(2)}
						</output>
					</div>
					<span class="hint">Review's "Accept all above threshold" uses this value.</span>
				</div>

				<span class="hint" data-testid="extraction-key-note">
					The API key is stored in the local Atlas database, sent only to the base URL above,
					and never logged.
				</span>

				{#if saveError}
					<p class="bad" role="alert" data-testid="settings-error">{saveError}</p>
				{/if}
			</div>
		</section>

		<section class="card" id="board-stages">
			<div class="card-head"><span class="card-title">Board stages</span></div>
			<div class="card-body">
				<span class="hint">
					The columns every project uses unless it sets its own. Renaming a stage here moves
					the tasks standing in it.
				</span>
				{#if stagesError}
					<p class="bad" role="alert" data-testid="board-stages-error">{stagesError}</p>
				{/if}
				<StageEditor {stages} onsave={saveStages} />
			</div>
		</section>

		<section class="card" id="mcp">
			<div class="card-head">
				<span class="card-title">MCP server</span>
				<Badge tone={online ? 'success' : 'neutral'} icon={online ? 'circle-check' : 'circle'}>
					{online ? 'running' : 'offline'}
				</Badge>
			</div>

			<div class="card-body">
				<div class="group">
					<span class="group-heading">Transports</span>
					<div class="transport">
						<span class="transport-name">stdio</span>
						<span class="mono value">atlas mcp</span>
						<span class="spacer"></span>
						<Badge>default</Badge>
					</div>
					<div class="transport">
						<span class="transport-name">HTTP</span>
						<span class="mono value">{httpEndpoint}</span>
						<span class="spacer"></span>
						<Badge>loopback only</Badge>
					</div>
				</div>

				<div class="group">
					<span class="group-heading">Connect</span>

					<div class="snippet">
						<pre class="mono">{CLAUDE_SNIPPET}</pre>
						<Button
							variant="ghost"
							size="sm"
							data-testid="mcp-copy-claude"
							onclick={() => copy('claude', CLAUDE_SNIPPET)}
						>
							{copied === 'claude' ? 'Copied' : 'Copy'}
						</Button>
					</div>

					<div class="snippet">
						<pre class="mono">{CODEX_SNIPPET}</pre>
						<Button
							variant="ghost"
							size="sm"
							data-testid="mcp-copy-codex"
							onclick={() => copy('codex', CODEX_SNIPPET)}
						>
							{copied === 'codex' ? 'Copied' : 'Copy'}
						</Button>
					</div>
				</div>

				<span class="hint">The full tools table arrives with the MCP settings phase.</span>
			</div>
		</section>

		<div class="foot">
			<Button variant="primary" data-testid="settings-save" disabled={saving} onclick={save}>
				{saving ? 'Saving…' : 'Save'}
			</Button>
			<Button onclick={reload} disabled={saving || settings.loading}>Reload</Button>
		</div>
	</div>
{/if}

<style>
	.title-row {
		display: flex;
		align-items: center;
		height: 28px;
		flex: 0 0 28px;
	}

	.title {
		font-size: 15px;
		font-weight: 600;
	}

	/* Frame 10's content panel scrolls; the shell's panel never does. */
	.pane {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.card {
		flex: 0 0 auto;
		overflow: hidden;
	}

	.card-head {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.card-title {
		font-weight: 600;
	}

	.card-body {
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}

	.pair {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.label {
		color: var(--text-secondary);
	}

	.slider {
		display: flex;
		align-items: center;
		gap: 10px;
		height: 28px;
	}

	.slider input {
		flex: 1;
		accent-color: var(--accent);
	}

	.slider output {
		min-width: 4ch;
		text-align: right;
	}

	.group {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.transport {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 22px;
		color: var(--text-secondary);
	}

	.transport-name {
		width: 70px;
	}

	.value {
		color: var(--text-primary);
	}

	.snippet {
		display: flex;
		align-items: flex-start;
		gap: 8px;
	}

	.snippet pre {
		flex: 1;
		margin: 0;
		padding: 6px 8px;
		background: var(--bg-base);
		border: 1px solid var(--border-default);
		border-radius: 3px;
		font-size: 12px;
		line-height: 18px;
		color: var(--text-secondary);
		white-space: pre;
		overflow-x: auto;
	}

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

	/* Same size as the hint it sits under; only the colour says it is a warning. */
	.hint.warn {
		color: var(--danger-text);
	}

	.test-result {
		color: var(--accent);
	}

	.test-result.bad {
		color: var(--danger-text);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}

	.foot {
		display: flex;
		gap: 8px;
		flex: 0 0 auto;
		padding-bottom: 4px;
	}
</style>
