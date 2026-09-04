<script lang="ts">
	// What the `/mcp` view used to be in full: Atlas's own MCP server, now the body of the
	// Atlas row's detail panel. Same data (`GET /api/v1/mcp/status`), same actions — copy
	// both connect snippets, restart the daemon, toggle a tool globally.
	import { onMount } from 'svelte';
	import { Badge, Button, Checkbox, Icon, Table, type TableColumn } from '$lib/ds';
	import { daemon } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { CLAUDE_SNIPPET, CODEX_SNIPPET, toolIcon, toolPluginId } from '$lib/mcp';
	import { copyText, inTauri } from '$lib/shell';
	import { loadMcp, mcp, toggleTool } from '$lib/stores/mcp.svelte';
	import type { McpClient, McpToolRow } from '$lib/types';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		/** `tools`, `resources`, `prompts` or `clients`: the section to scroll to once the
		 * report has loaded, set by a side panel jump row. */
		section?: string | null;
	}

	let { section = null }: Props = $props();

	let restarting = $state(false);
	let togglingTool = $state<string | null>(null);
	let copied = $state<string | null>(null);

	const countsText = $derived(
		mcp.report
			? `${mcp.report.counts.tools} tools · ${mcp.report.counts.resources} resources · ${mcp.report.counts.prompts} prompts`
			: '…'
	);
	const connectSnippet = `${CLAUDE_SNIPPET}\n\n${CODEX_SNIPPET}`;

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

	async function toggle(row: McpToolRow): Promise<void> {
		togglingTool = row.name;
		try {
			await toggleTool(row);
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			togglingTool = null;
		}
	}

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

	const COPIED_MS = 1500;
	let copiedTimer: ReturnType<typeof setTimeout> | null = null;

	async function copy(label: string, text: string): Promise<void> {
		try {
			await copyText(text);
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

	onMount(() => {
		if (!mcp.report && !mcp.loading) void loadMcp();
	});

	// A side panel row's jump names a section; scroll to it once the report has loaded
	// and the section exists to scroll to.
	$effect(() => {
		if (!section || !mcp.report) return;
		document.getElementById(section)?.scrollIntoView({ behavior: 'instant', block: 'start' });
	});
</script>

<div class="atlas" data-testid="mcp-atlas-detail">
	{#if mcp.error}
		<p class="bad" role="alert" data-testid="mcp-error">{mcp.error}</p>
	{/if}

	<div class="actions">
		<span class="mono count" data-testid="mcp-counts">{countsText}</span>
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

	<div class="group">
		<span class="group-heading">Transports</span>
		<div class="transport">
			<span class="transport-name">stdio</span>
			<span class="mono value">{mcp.report?.transports.stdio.command ?? 'atlas mcp'}</span>
			<span class="spacer"></span>
			<Badge>default</Badge>
		</div>
		<div class="transport">
			<span class="transport-name">HTTP</span>
			<span class="mono value">
				{mcp.report?.transports.http.url ?? `http://127.0.0.1:${daemon.port}/mcp`}
			</span>
			<span class="spacer"></span>
			<Badge>loopback only</Badge>
		</div>
		<div class="transport" data-testid="mcp-protocol">
			<span class="transport-name">Protocol</span>
			<span class="mono value">{mcp.report?.transports.http.protocol_version ?? '…'}</span>
			<span class="spacer"></span>
			<span class="hint">tools · resources · prompts</span>
		</div>
	</div>

	<div class="group">
		<span class="group-heading">Connect</span>
		<pre class="mono connect-snippet">{connectSnippet}</pre>
	</div>

	<div class="group" id="tools">
		<span class="group-heading">Tools</span>
		<div class="tools-table" data-testid="mcp-tools">
			<Table
				id="mcp-tools"
				columns={toolColumns}
				rows={mcp.report?.tools ?? []}
				rowKey={(t: McpToolRow) => t.name}
			>
				{#snippet cell(toolRow: McpToolRow, column: TableColumn<McpToolRow>)}
					{#if column.key === 'name'}
						<span class="tool-name">
							<Icon name={toolIcon(toolRow.name)} size={12} color="var(--text-tertiary)" />
							<span class="mono">{toolRow.name}</span>
							{#if toolPluginId(toolRow.source)}
								<Badge variant="outline" data-testid="mcp-tool-source-{toolRow.name}">
									Plugin {toolPluginId(toolRow.source)}
								</Badge>
							{/if}
						</span>
					{:else if column.key === 'description'}
						{toolRow.description}
					{:else if column.key === 'args'}
						{toolRow.args}
					{:else if column.key === 'scope'}
						<Badge tone={toolRow.scope === 'write' ? 'warning' : 'info'}>{toolRow.scope}</Badge>
					{:else}
						<Checkbox
							checked={toolRow.enabled}
							disabled={togglingTool === toolRow.name}
							aria-label={`Enable ${toolRow.name}`}
							data-testid={`mcp-tool-toggle-${toolRow.name}`}
							onchange={() => toggle(toolRow)}
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
			answers "method not found". Write tools still respect a project's agent access rules. A tool
			a plugin contributes carries a <strong>Plugin</strong> badge and is answered by the desktop
			app, so it is listed only while Atlas is running.
		</span>
	</div>

	<div class="group" id="resources">
		<span class="group-heading">Resources</span>
		<div class="resource-list">
			{#each mcp.report?.resources ?? [] as resource (resource.uri)}
				<div class="resource-row">
					<span class="mono value">{resource.uri}</span>
					<span class="hint">{resource.description ?? ''}</span>
				</div>
			{/each}
		</div>
	</div>

	<div class="group" id="prompts">
		<span class="group-heading">Prompts</span>
		<div class="resource-list">
			{#each mcp.report?.prompts ?? [] as prompt (prompt.name)}
				<div class="resource-row">
					<span class="mono value">{prompt.name}</span>
					<span class="hint">{prompt.description ?? ''}</span>
				</div>
			{/each}
		</div>
	</div>

	<div class="group" id="clients">
		<span class="group-heading">Connected clients</span>
		<div class="clients-table" data-testid="mcp-clients">
			<Table
				id="mcp-clients"
				columns={clientColumns}
				rows={mcp.report?.clients ?? []}
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
			A stdio client's call count refreshes on its 60 s heartbeat, so it can lag behind the calls
			it has actually made.
		</span>
	</div>
</div>

<style>
	.atlas {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.actions {
		display: flex;
		align-items: center;
		gap: 6px;
		flex-wrap: wrap;
	}

	.count {
		color: var(--text-tertiary);
		font-size: 11px;
	}

	.spacer {
		flex: 1;
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
		min-height: 22px;
		color: var(--text-secondary);
	}

	.transport-name {
		width: 70px;
		flex: 0 0 70px;
	}

	.resource-list {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.resource-row {
		display: flex;
		flex-direction: column;
		color: var(--text-secondary);
	}

	.resource-row .hint {
		white-space: normal;
	}

	.value {
		color: var(--text-primary);
		word-break: break-all;
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

	.tool-name {
		display: flex;
		align-items: center;
		gap: 6px;
	}

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

	.mono {
		font-family: var(--font-mono);
		font-size: 12px;
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
</style>
