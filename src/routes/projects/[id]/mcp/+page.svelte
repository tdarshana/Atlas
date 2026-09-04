<script lang="ts">
	// The project MCP tab: every MCP server configured for this project (Claude Code's
	// project and local entries, Codex's and Cursor's project files, the plugin servers
	// and Atlas), grouped by where they came from. A row opens the detail panel; the
	// Atlas row's detail is what this tab used to be in full (the project connect
	// snippet, agent access, resources, the tools table and the clients).
	import { Button } from '$lib/ds';
	import AddServerDialog from '$lib/components/mcp/AddServerDialog.svelte';
	import AtlasProjectDetail from '$lib/components/mcp/AtlasProjectDetail.svelte';
	import RemoveServerDialog from '$lib/components/mcp/RemoveServerDialog.svelte';
	import ServerDetail from '$lib/components/mcp/ServerDetail.svelte';
	import ServerTable from '$lib/components/mcp/ServerTable.svelte';
	import { errorMessage } from '$lib/errors';
	import { projectConnectSnippet } from '$lib/mcp';
	import { groupByScope, NO_PROJECT_SERVERS } from '$lib/mcp-servers';
	import { copyText } from '$lib/shell';
	import { project, setHeaderActions } from '$lib/stores/project.svelte';
	import {
		add,
		check,
		loadServers,
		openServer,
		remove,
		servers,
		setDetailWidth,
		setEnabled
	} from '$lib/stores/mcp-servers.svelte';
	import type { McpServerEntry, NewMcpServer } from '$lib/types';
	import { push } from '$lib/ui/toasts.svelte';

	const id = $derived(project.current?.id ?? '');
	const rootPath = $derived(project.current?.root_path ?? '');

	let adding = $state(false);
	let removing = $state<McpServerEntry | null>(null);
	let copied = $state(false);
	let copyTimer: ReturnType<typeof setTimeout> | null = null;

	// `Project` is kept even when it is empty: a repo with no project-level server should
	// still be told where `Add server…` would write.
	const groups = $derived(groupByScope(servers.items, true));
	const open = $derived(servers.items.find((s) => s.id === servers.openId) ?? null);
	const connectSnippet = $derived(projectConnectSnippet(rootPath));

	async function runCheck(row: McpServerEntry): Promise<void> {
		try {
			const result = await check(row.id);
			if (!result.ok) push('error', result.error ?? `${row.name} did not answer`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	async function toggle(row: McpServerEntry, enabled: boolean): Promise<void> {
		try {
			await setEnabled(row.id, enabled);
		} catch (e) {
			push('error', errorMessage(e));
			await loadServers(servers.projectId);
		}
	}

	async function addServer(input: NewMcpServer): Promise<void> {
		await add(input);
		push('success', 'MCP server added');
	}

	async function copySnippet(): Promise<void> {
		try {
			await copyText(connectSnippet);
			copied = true;
			if (copyTimer !== null) clearTimeout(copyTimer);
			copyTimer = setTimeout(() => {
				copyTimer = null;
				copied = false;
			}, 1500);
		} catch (e) {
			push('error', `Could not copy: ${errorMessage(e)}`);
		}
	}

	$effect(() => () => {
		if (copyTimer !== null) clearTimeout(copyTimer);
	});

	$effect(() => {
		if (!id) return;
		void loadServers(id);
	});

	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});
</script>

{#snippet headerActions()}
	<Button variant="ghost" size="sm" data-testid="project-mcp-copy-connect" onclick={copySnippet}>
		{copied ? 'Copied' : 'Copy connect snippet'}
	</Button>
	<Button size="sm" data-testid="project-mcp-add-server" onclick={() => (adding = true)}>
		Add server…
	</Button>
{/snippet}

<div class="pane" data-testid="project-mcp-page">
	{#if servers.error}
		<p class="bad" role="alert" data-testid="mcp-servers-error">{servers.error}</p>
	{/if}

	{#if servers.warnings.length > 0}
		<span class="hint" data-testid="mcp-servers-warnings">{servers.warnings.join(' · ')}</span>
	{/if}

	<div class="split">
		<div class="groups">
			{#each groups as group (group.label)}
				<div class="group">
					<span class="group-heading" data-testid="project-mcp-group">{group.label}</span>
					<ServerTable
						id="project-mcp-servers-{group.label.toLowerCase()}"
						rows={group.servers}
						checks={servers.checks}
						loading={servers.loading}
						selectedId={servers.openId}
						busyId={servers.busyId}
						enabledLabel="Enabled here"
						emptyText={group.label === 'Project' ? NO_PROJECT_SERVERS : 'No servers here.'}
						onopen={(row) => openServer(row.id)}
						oncheck={runCheck}
						ontoggle={toggle}
						onremove={(row) => (removing = row)}
					/>
				</div>
			{/each}
		</div>

		{#if open}
			<ServerDetail
				server={open}
				check={servers.checks[open.id] ?? null}
				width={servers.detailWidth}
				busy={servers.busyId === open.id}
				onclose={() => openServer(null)}
				onresize={setDetailWidth}
				oncheck={runCheck}
				ontoggle={toggle}
				onremove={(row) => (removing = row)}
			>
				{#snippet atlas()}
					{#if open?.is_atlas && id}
						<AtlasProjectDetail {id} />
					{/if}
				{/snippet}
			</ServerDetail>
		{/if}
	</div>
</div>

<AddServerDialog
	open={adding}
	projectId={id || null}
	onclose={() => (adding = false)}
	onadd={addServer}
/>

<RemoveServerDialog server={removing} onclose={() => (removing = null)} onremove={remove} />

<style>
	.pane {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.split {
		flex: 1;
		min-height: 0;
		display: flex;
		gap: 12px;
	}

	/* See the global view: `overflow-x` is pinned so the vertical scroll does not bring a
	   horizontal scrollbar with it, and this region is never re-anchored by the detail. */
	.groups {
		flex: 1;
		min-width: 0;
		min-height: 0;
		overflow-y: auto;
		overflow-x: hidden;
		overflow-anchor: none;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

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
</style>
