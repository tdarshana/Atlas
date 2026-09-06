<script lang="ts">
	// Settings (frame 10): the section list. Each card is a component under
	// `$lib/components/settings` that owns its own state; this route keeps the extraction
	// form and its Save footer (the one flow that spans two cards, since a saved key is
	// mirrored into the vault), the anchor navigation, and `reload`, which refreshes the
	// sections in order on load and on Reload. Save sends only the keys that changed, and
	// the API key only when one was typed: the server hands back "***" for a stored key
	// and reads a missing key as "leave it alone".
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { Button, Checkbox, IconButton, Input } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { inTauri, setStatusItems } from '$lib/shell';
	import { mirrorKey, vaultHintText } from '$lib/shell/vault';
	import { scrollToSection } from '$lib/components/settings-sections';
	import About from '$lib/components/settings/About.svelte';
	import Appearance from '$lib/components/settings/Appearance.svelte';
	import BoardStages from '$lib/components/settings/BoardStages.svelte';
	import Cli from '$lib/components/settings/Cli.svelte';
	import Daemon from '$lib/components/settings/Daemon.svelte';
	import McpServers from '$lib/components/settings/McpServers.svelte';
	import Notifications from '$lib/components/settings/Notifications.svelte';
	import Permissions from '$lib/components/settings/Permissions.svelte';
	import Plugins from '$lib/components/settings/Plugins.svelte';
	import SettingsCard from '$lib/components/settings/SettingsCard.svelte';
	import Shortcuts from '$lib/components/settings/Shortcuts.svelte';
	import Skills from '$lib/components/settings/Skills.svelte';
	import Vault from '$lib/components/settings/Vault.svelte';
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
	import type { ExtractionTestResult, VaultStatus } from '$lib/types';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import { push } from '$lib/platform/toasts.svelte';

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

	/** The vault's status as the Security card last read it. */
	let vault = $state<VaultStatus>('missing');

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

	// The sections `reload` refreshes, in the order they used to load.
	let daemonSection = $state.raw<Daemon>();
	let cliSection = $state.raw<Cli>();
	let boardStages = $state.raw<BoardStages>();
	let appearance = $state.raw<Appearance>();
	let mcpServers = $state.raw<McpServers>();
	let notifications = $state.raw<Notifications>();
	let vaultSection = $state.raw<Vault>();
	let about = $state.raw<About>();

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
		// Captured before `syncDraft` blanks the box again: a stored key never reads back,
		// so this is the only moment the real value is still around to mirror.
		const savedKey = typeof partial['extraction.api_key'] === 'string' ? partial['extraction.api_key'] : null;
		saving = true;
		saveError = null;
		try {
			await saveSettings(partial);
			syncDraft();
			push('success', 'Settings saved');
			if (savedKey && vault === 'unlocked') {
				try {
					await mirrorKey('global', savedKey);
					await vaultSection?.refresh();
				} catch (e) {
					push('error', `Vault: ${errorMessage(e)}`);
				}
			}
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
		appearance?.refresh();
		await boardStages?.refresh();
		await mcpServers?.refresh();
		await daemonSection?.refresh();
		await cliSection?.refresh();
		await about?.refresh();
		await vaultSection?.refresh();
		await notifications?.refresh();
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
		<Daemon bind:this={daemonSection} />
		<Cli bind:this={cliSection} />

		<SettingsCard id="extraction" title="Extraction">
			{#snippet head()}
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
			{/snippet}

			<Checkbox
				label="Enable extraction"
				bind:checked={enabled}
				data-testid="settings-enabled"
				onreset={enabled ? () => (enabled = false) : undefined}
			/>
			{#if enabled && baseUrl.trim() === ''}
				<span class="hint warn" role="status" data-testid="settings-extraction-warning">
					Enabled, but no base URL is set: nothing will run until one is.
				</span>
			{/if}

			<div class="pair">
				<Input
					label="Base URL"
					mono
					bind:value={baseUrl}
					data-testid="settings-base-url"
					placeholder="https://api.deepseek.com"
					onreset={baseUrl ? () => (baseUrl = '') : undefined}
				/>
				<Input
					label="Model"
					mono
					bind:value={model}
					data-testid="settings-model"
					placeholder="deepseek-chat"
					onreset={model ? () => (model = '') : undefined}
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
			{#if inTauri() && vaultHintText(vault)}
				<span class="hint" role="status" data-testid="settings-vault-hint">
					{vaultHintText(vault)}
				</span>
			{/if}

			<div class="field">
				<span class="label dbm-field__labelrow">
					Auto-accept threshold
					{#if Math.abs(threshold - DEFAULT_MIN_CONFIDENCE) > 1e-9}
						<IconButton
							size="sm"
							icon="undo-2"
							label="Reset auto-accept threshold"
							data-testid="settings-threshold-reset"
							onclick={() => (threshold = DEFAULT_MIN_CONFIDENCE)}
						/>
					{/if}
				</span>
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
		</SettingsCard>

		<BoardStages bind:this={boardStages} />
		<Appearance bind:this={appearance} />
		<McpServers bind:this={mcpServers} />
		<Plugins />
		<Permissions />
		<Skills />
		<Shortcuts />
		<Notifications bind:this={notifications} />
		<Vault bind:this={vaultSection} onstatus={(status) => (vault = status)} />
		<About bind:this={about} />

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
