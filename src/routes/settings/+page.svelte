<script lang="ts">
	// Settings (frame 10): the seven keys the daemon stores, the global board stages, and
	// how agents reach the MCP server. Port and embedding model are read only because the
	// daemon sets them at startup. Save sends only the keys that changed, and the API key
	// only when one was typed: the server hands back "***" for a stored key and reads a
	// missing key as "leave it alone".
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { Badge, Button, Checkbox, Icon, Input, Table, type TableColumn } from '$lib/ds';
	import { api, daemon } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { inTauri, setStatusItems } from '$lib/shell';
	import { CLAUDE_SNIPPET, CODEX_SNIPPET, nextDisabledTools, toolIcon } from '$lib/mcp';
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
	import type {
		ExtractionTestResult,
		McpClient,
		McpStatusReport,
		McpToolRow,
		Stage
	} from '$lib/types';
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

	/** The CONNECT block shows both snippets, one after the other, as frame 10 does. */
	const connectSnippet = `${CLAUDE_SNIPPET}\n\n${CODEX_SNIPPET}`;

	/** `GET /api/v1/mcp/status`: transports, counts, tools, resources, prompts, clients. */
	let mcp = $state<McpStatusReport | null>(null);
	let mcpError = $state<string | null>(null);
	let restarting = $state(false);
	let togglingTool = $state<string | null>(null);

	const mcpCountsText = $derived(
		mcp ? `${mcp.counts.tools} tools · ${mcp.counts.resources} resources · ${mcp.counts.prompts} prompts` : '…'
	);

	const toolColumns: TableColumn<McpToolRow>[] = [
		{ key: 'name', label: 'Name', width: '220px', mono: true, sortable: true },
		{ key: 'description', label: 'Description' },
		{ key: 'args', label: 'Arguments', width: '260px', mono: true },
		{ key: 'scope', label: 'Scope', width: '80px', sortable: true },
		{ key: 'enabled', label: 'Enable', width: '70px' }
	];

	const clientColumns: TableColumn<McpClient>[] = [
		{ key: 'client_name', label: 'Client', mono: true, sortable: true },
		{ key: 'transport', label: 'Transport', width: '90px', sortable: true },
		{ key: 'last_seen', label: 'Last seen', width: '110px', sortable: true },
		{ key: 'tool_calls', label: 'Calls', width: '70px', align: 'right', mono: true, sortable: true }
	];

	async function loadMcp(): Promise<void> {
		try {
			mcp = await api().mcpStatus();
			mcpError = null;
		} catch (e) {
			mcpError = errorMessage(e);
		}
	}

	/** Flips one tool's checkbox: writes `mcp.disabled_tools` with that name added or
	 * removed, leaving every other disabled tool untouched, then reloads the status. */
	async function toggleTool(row: McpToolRow): Promise<void> {
		if (!mcp) return;
		const currentlyDisabled = mcp.tools.filter((t) => !t.enabled).map((t) => t.name);
		const next = nextDisabledTools(currentlyDisabled, row.name, !row.enabled);
		togglingTool = row.name;
		try {
			await api().setSettings({ 'mcp.disabled_tools': next });
			await loadMcp();
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			togglingTool = null;
		}
	}

	/** Stop and start the daemon through the Tauri `daemon_restart` command. Only
	 * available inside the desktop app; the browser has no way to manage the process. */
	async function restartDaemon(): Promise<void> {
		if (!inTauri()) return;
		restarting = true;
		try {
			const { invoke } = await import('@tauri-apps/api/core');
			await invoke('daemon_restart');
			push('success', 'MCP server restarted');
			await loadMcp();
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			restarting = false;
		}
	}

	/** How long the button says "Copied" before going back to its own name. */
	const COPIED_MS = 1500;
	let copiedTimer: ReturnType<typeof setTimeout> | null = null;

	/**
	 * Copies a snippet and names which one was copied, so the feedback is on the button.
	 * The label goes back to "Copy" shortly after: a button that stays "Copied" reads as
	 * its permanent name, and gives no feedback the second time it is pressed.
	 */
	async function copy(label: string, text: string): Promise<void> {
		try {
			await navigator.clipboard.writeText(text);
			copied = label;
			if (copiedTimer !== null) clearTimeout(copiedTimer);
			copiedTimer = setTimeout(() => {
				copiedTimer = null;
				copied = null;
			}, COPIED_MS);
		} catch (e) {
			push('error', `Could not copy: ${errorMessage(e)}`);
		}
	}

	$effect(() => () => {
		if (copiedTimer !== null) clearTimeout(copiedTimer);
	});

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
		await loadMcp();
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
				<span class="mono count" data-testid="mcp-counts">{mcpCountsText}</span>
				<span class="spacer"></span>
				<Button
					variant="ghost"
					size="sm"
					data-testid="mcp-copy-claude"
					onclick={() => copy('claude', CLAUDE_SNIPPET)}
				>
					{copied === 'claude' ? 'Copied' : 'Copy Claude command'}
				</Button>
				<Button
					variant="ghost"
					size="sm"
					data-testid="mcp-copy-codex"
					onclick={() => copy('codex', CODEX_SNIPPET)}
				>
					{copied === 'codex' ? 'Copied' : 'Copy Codex config'}
				</Button>
				<Button
					variant="ghost"
					size="sm"
					data-testid="mcp-restart"
					disabled={!inTauri() || restarting}
					title={inTauri() ? 'Stop and start the daemon' : 'Only available in the desktop app'}
					onclick={restartDaemon}
				>
					{restarting ? 'Restarting…' : 'Restart'}
				</Button>
			</div>

			<div class="card-body">
				{#if mcpError}
					<p class="bad" role="alert" data-testid="mcp-error">{mcpError}</p>
				{/if}

				<div class="pair">
					<div class="group">
						<span class="group-heading">Transports</span>
						<div class="transport">
							<span class="transport-name">stdio</span>
							<span class="mono value">{mcp?.transports.stdio.command ?? 'atlas mcp'}</span>
							<span class="spacer"></span>
							<Badge>default</Badge>
						</div>
						<div class="transport">
							<span class="transport-name">HTTP</span>
							<span class="mono value">
								{mcp?.transports.http.url ?? `http://127.0.0.1:${port}/mcp`}
							</span>
							<span class="spacer"></span>
							<Badge>loopback only</Badge>
						</div>
						<div class="transport" data-testid="mcp-protocol">
							<span class="transport-name">Protocol</span>
							<span class="mono value">{mcp?.transports.http.protocol_version ?? '…'}</span>
							<span class="spacer"></span>
							<span class="hint">tools · resources · prompts</span>
						</div>
					</div>

					<div class="group">
						<span class="group-heading">Connect</span>
						<pre class="mono connect-snippet">{connectSnippet}</pre>
					</div>
				</div>

				<div class="group">
					<span class="group-heading">Connected clients</span>
					<div class="clients-table" data-testid="mcp-clients">
						<Table
							id="mcp-clients"
							columns={clientColumns}
							rows={mcp?.clients ?? []}
							rowKey={(c: McpClient) => c.id}
						>
							{#snippet cell(clientRow: McpClient, column: TableColumn<McpClient>)}
								{#if column.key === 'last_seen'}
									{relativeAge(clientRow.last_seen)} ago
								{:else if column.key === 'transport'}
									{clientRow.transport}
								{:else if column.key === 'tool_calls'}
									{clientRow.tool_calls}
								{:else}
									{clientRow.client_name}
								{/if}
							{/snippet}
							{#snippet empty()}
								<span class="hint">No clients connected right now.</span>
							{/snippet}
						</Table>
					</div>
					<span class="hint">
						A stdio client's call count refreshes on its 60 s heartbeat, so it can lag behind
						the calls it has actually made.
					</span>
				</div>

				<div class="group">
					<span class="group-heading">Tools</span>
					<div class="tools-table" data-testid="mcp-tools">
						<Table
							id="mcp-tools"
							columns={toolColumns}
							rows={mcp?.tools ?? []}
							rowKey={(t: McpToolRow) => t.name}
						>
							{#snippet cell(toolRow: McpToolRow, column: TableColumn<McpToolRow>)}
								{#if column.key === 'name'}
									<span class="tool-name">
										<Icon name={toolIcon(toolRow.name)} size={12} color="var(--text-tertiary)" />
										<span class="mono">{toolRow.name}</span>
									</span>
								{:else if column.key === 'description'}
									{toolRow.description}
								{:else if column.key === 'args'}
									{toolRow.args}
								{:else if column.key === 'scope'}
									<Badge tone={toolRow.scope === 'write' ? 'warning' : 'info'}>
										{toolRow.scope}
									</Badge>
								{:else}
									<Checkbox
										checked={toolRow.enabled}
										disabled={togglingTool === toolRow.name}
										aria-label={`Enable ${toolRow.name}`}
										data-testid={`mcp-tool-toggle-${toolRow.name}`}
										onchange={() => toggleTool(toolRow)}
									/>
								{/if}
							{/snippet}
							{#snippet empty()}
								<span class="hint">No tools reported yet.</span>
							{/snippet}
						</Table>
					</div>
					<span class="hint">
						Disabled tools are absent from <span class="mono">tools/list</span> and a call to one
						answers "method not found". Write tools still respect a project's agent access rules.
					</span>
				</div>

				<div class="pair">
					<div class="group">
						<span class="group-heading">Resources</span>
						<div class="resource-list">
							{#each mcp?.resources ?? [] as resource (resource.uri)}
								<div class="resource-row">
									<span class="mono value">{resource.uri}</span>
									<span class="hint">{resource.description ?? ''}</span>
								</div>
							{/each}
						</div>
					</div>
					<div class="group">
						<span class="group-heading">Prompts</span>
						<div class="resource-list">
							{#each mcp?.prompts ?? [] as prompt (prompt.name)}
								<div class="resource-row">
									<span class="mono value">{prompt.name}</span>
									<span class="hint">{prompt.description ?? ''}</span>
								</div>
							{/each}
						</div>
					</div>
				</div>
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

	/* RESOURCES and PROMPTS rows: a URI or prompt name can run much longer than a
	   transport value, and its description can too, so each row is its own two-column
	   grid (name/URI, then description) instead of sharing `.transport`'s fixed-height
	   flex row, which wrapped and overlapped once either side got long. */
	.resource-list {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.resource-row {
		display: grid;
		grid-template-columns: minmax(160px, 260px) 1fr;
		gap: 12px;
		align-items: baseline;
		color: var(--text-secondary);
	}

	.resource-row .hint {
		white-space: normal;
	}

	.value {
		color: var(--text-primary);
	}

	.connect-snippet {
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

	.count {
		color: var(--text-tertiary);
	}

	.tool-name {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	/* Bounds the tools table so 29 rows scroll inside the card instead of stretching it
	   past the pane; the clients table stays natural height since it is usually short. */
	.tools-table {
		height: 320px;
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		overflow: hidden;
	}

	.clients-table {
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		overflow: hidden;
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
