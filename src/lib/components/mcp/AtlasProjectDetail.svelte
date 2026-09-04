<script lang="ts">
	// What the project MCP tab used to be in full: this project's own view of Atlas's
	// server, now the body of the Atlas row's detail panel. Same data
	// (`GET /api/v1/projects/{id}/mcp`) — the project-scoped connect snippet, the agent
	// access summary, this project's `atlas://` resources, the tools table with its
	// per-tool `Enabled here` override, and the clients whose last call resolved here.
	import { Badge, Button, Checkbox, Table, type TableColumn } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { agentAccessSummary } from '$lib/components/project/mcp';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { nextDisabledTools, projectConnectSnippet, projectToolState, toolPluginId } from '$lib/mcp';
	import { loadProject } from '$lib/stores/projects.svelte';
	import { project } from '$lib/stores/project.svelte';
	import type { McpClient, ProjectMcpReport, ProjectMcpToolRow, Uuid } from '$lib/types';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		id: Uuid;
		/** `tools`, `resources` or `clients`: the section a side panel jump row named. */
		section?: string | null;
	}

	let { id, section = null }: Props = $props();

	let report = $state<ProjectMcpReport | null>(null);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let togglingTool = $state<string | null>(null);

	const toolColumns: TableColumn<ProjectMcpToolRow>[] = [
		{ key: 'name', label: 'Name', width: '200px', mono: true, sortable: true },
		{ key: 'description', label: 'Description' },
		{ key: 'args', label: 'Arguments', width: '220px', mono: true },
		{ key: 'scope', label: 'Scope', width: '80px', sortable: true },
		{ key: 'enabled_here', label: 'Enabled here', width: '110px' }
	];

	const clientColumns: TableColumn<McpClient>[] = [
		{ key: 'client_name', label: 'Client', mono: true, sortable: true },
		{ key: 'transport', label: 'Transport', width: '90px', sortable: true },
		{ key: 'last_seen', label: 'Last seen', width: '110px', sortable: true },
		{ key: 'tool_calls', label: 'Calls', width: '70px', align: 'right', mono: true, sortable: true }
	];

	const access = $derived(project.current ? agentAccessSummary(project.current.agent_access) : null);
	const connectSnippet = $derived(
		projectConnectSnippet(report?.connect.project_root ?? project.current?.root_path ?? '')
	);

	async function load(): Promise<void> {
		if (!id) return;
		loading = true;
		try {
			report = await api().projectMcp(id);
			error = null;
		} catch (e) {
			report = null;
			error = errorMessage(e);
		} finally {
			loading = false;
		}
	}

	/** Writes the project's own `mcp_disabled_tools` with this tool's name added or
	 * removed, off `project.current`'s own list so a redundant globally-disabled entry
	 * is never dropped by accident. A globally disabled row is not editable here. */
	async function toggle(row: ProjectMcpToolRow): Promise<void> {
		if (!id || projectToolState(row) === 'disabled_globally') return;
		const current = project.current?.mcp_disabled_tools ?? [];
		const next = nextDisabledTools(current, row.name, !row.enabled_here);
		togglingTool = row.name;
		try {
			await api().setProjectMcpTools(id, next);
			await Promise.all([load(), loadProject(id)]);
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			togglingTool = null;
		}
	}

	async function resetToGlobal(): Promise<void> {
		if (!id) return;
		try {
			await api().setProjectMcpTools(id, []);
			await Promise.all([load(), loadProject(id)]);
			push('success', 'Reset to the global tool list');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	$effect(() => {
		if (!id) return;
		void load();
	});

	$effect(() => {
		if (!section || !report) return;
		document.getElementById(section)?.scrollIntoView({ behavior: 'instant', block: 'start' });
	});
</script>

<div class="atlas" data-testid="project-mcp-atlas-detail">
	{#if error}
		<p class="bad" role="alert" data-testid="project-mcp-error">{error}</p>
	{/if}

	<div class="group">
		<span class="group-heading">Connect</span>
		<pre class="mono connect-snippet">{connectSnippet}</pre>
	</div>

	{#if access}
		<div class="group">
			<span class="group-heading">Agent access</span>
			<div class="reading">
				<span class="label">Memory writers</span>
				<span class="value">{access.memoryWriters}</span>
			</div>
			<div class="reading">
				<span class="label">Task movers</span>
				<span class="value">{access.taskMovers}</span>
			</div>
			<div class="reading">
				<span class="label">Require review</span>
				<span class="value">{access.requireReview ? 'Yes' : 'No'}</span>
			</div>
			<a href="/projects/{id}/settings" data-testid="project-mcp-agent-access-link">
				Project settings
			</a>
		</div>
	{/if}

	<div class="group" id="resources">
		<span class="group-heading">Resources</span>
		<div class="resource-list">
			{#each report?.resources ?? [] as resource (resource.uri)}
				<div class="resource-row">
					<span class="mono value">{resource.uri}</span>
					<span class="hint">{resource.description ?? ''}</span>
				</div>
			{/each}
		</div>
	</div>

	<div class="group" id="tools">
		<div class="head">
			<span class="group-heading">Tools</span>
			<span class="spacer"></span>
			<Button variant="ghost" size="sm" data-testid="project-mcp-reset" onclick={resetToGlobal}>
				Reset to global
			</Button>
		</div>
		<div class="tools-table" data-testid="project-mcp-tools">
			<Table
				id="project-mcp-tools"
				columns={toolColumns}
				rows={report?.tools ?? []}
				rowKey={(t: ProjectMcpToolRow) => t.name}
			>
				{#snippet cell(row: ProjectMcpToolRow, column: TableColumn<ProjectMcpToolRow>)}
					{#if column.key === 'description'}
						{row.description}
					{:else if column.key === 'args'}
						{row.args}
					{:else if column.key === 'scope'}
						<Badge tone={row.scope === 'write' ? 'warning' : 'info'}>{row.scope}</Badge>
					{:else if column.key === 'enabled_here'}
						{#if projectToolState(row) === 'disabled_globally'}
							<Checkbox
								checked={false}
								disabled
								title="disabled for every project in Settings"
								aria-label={`${row.name} is disabled for every project in Settings`}
								data-testid={`project-mcp-tool-toggle-${row.name}`}
							/>
						{:else}
							<Checkbox
								checked={row.enabled_here}
								disabled={togglingTool === row.name}
								aria-label={`Enable ${row.name} here`}
								data-testid={`project-mcp-tool-toggle-${row.name}`}
								onchange={() => toggle(row)}
							/>
						{/if}
					{:else}
						{row.name}
						{#if toolPluginId(row.source)}
							<Badge variant="outline" data-testid="project-mcp-tool-source-{row.name}">
								Plugin {toolPluginId(row.source)}
							</Badge>
						{/if}
					{/if}
				{/snippet}
				{#snippet empty()}
					<span class="hint">{loading ? 'Loading…' : 'No tools reported yet.'}</span>
				{/snippet}
			</Table>
		</div>
	</div>

	<div class="group" id="clients">
		<span class="group-heading">Connected clients (HTTP sessions)</span>
		<div class="clients-table" data-testid="project-mcp-clients">
			<Table
				id="project-mcp-clients"
				columns={clientColumns}
				rows={report?.clients ?? []}
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
					<span class="hint">No HTTP client has called into this project yet.</span>
				{/snippet}
			</Table>
		</div>
	</div>
</div>

<style>
	.atlas {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.group {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.head {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.spacer {
		flex: 1;
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

	.reading {
		display: flex;
		align-items: baseline;
		gap: 12px;
		color: var(--text-secondary);
	}

	.label {
		width: 120px;
		flex: 0 0 120px;
	}

	.value {
		color: var(--text-primary);
		word-break: break-all;
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

	.tools-table {
		height: 280px;
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
