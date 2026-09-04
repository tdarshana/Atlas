<script lang="ts">
	// The MCP servers view: every MCP server the user's agents are wired to, discovered
	// from those agents' own config files, with Atlas as one row among them. A row opens
	// the detail panel on the right; the Atlas row's detail is what this page used to be
	// in full (its transports, tools, resources, prompts and connected clients).
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { Button } from '$lib/ds';
	import AddServerDialog from '$lib/components/mcp/AddServerDialog.svelte';
	import AtlasServerDetail from '$lib/components/mcp/AtlasServerDetail.svelte';
	import RemoveServerDialog from '$lib/components/mcp/RemoveServerDialog.svelte';
	import ServerDetail from '$lib/components/mcp/ServerDetail.svelte';
	import ServerTable from '$lib/components/mcp/ServerTable.svelte';
	import { errorMessage } from '$lib/errors';
	import { enabledSummary } from '$lib/mcp-servers';
	import { setStatusItems } from '$lib/shell';
	import {
		add,
		check,
		checkAll,
		loadServers,
		openServer,
		remove,
		servers,
		setDetailWidth,
		setEnabled
	} from '$lib/stores/mcp-servers.svelte';
	import type { McpServerEntry, NewMcpServer } from '$lib/types';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	let adding = $state(false);
	let removing = $state<McpServerEntry | null>(null);
	let checkingAll = $state(false);

	const rows = $derived(
		servers.sourceFilter
			? servers.items.filter((s) => s.source === servers.sourceFilter)
			: servers.items
	);
	const summary = $derived(enabledSummary(servers.items));
	const open = $derived(servers.items.find((s) => s.id === servers.openId) ?? null);

	// A side panel ATLAS row's jump lands here as a hash; it opens the Atlas row's detail
	// at that section rather than scrolling this page.
	const section = $derived(page.url.hash.replace(/^#/, '') || null);

	onMount(() => {
		void loadServers(null);
	});

	// Once per hash, not once per list reload: a toggle's reload must not drag the panel
	// back to the Atlas row the user has since navigated away from.
	let openedForHash: string | null = null;

	$effect(() => {
		if (!section || openedForHash === section) return;
		const atlas = servers.items.find((s) => s.is_atlas);
		if (!atlas) return;
		openedForHash = section;
		openServer(atlas.id);
	});

	async function runCheck(row: McpServerEntry): Promise<void> {
		try {
			const result = await check(row.id);
			if (!result.ok) push('error', result.error ?? `${row.name} did not answer`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	async function runCheckAll(): Promise<void> {
		checkingAll = true;
		try {
			await checkAll();
		} finally {
			checkingAll = false;
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

	$effect(() => {
		setStatusItems({ right: [{ text: `${servers.items.length} servers` }] });
	});
</script>

<div class="title-row">
	<span class="title">MCP servers</span>
	<span class="spacer"></span>
	<Button variant="ghost" size="sm" data-testid="mcp-check-all" disabled={checkingAll} onclick={runCheckAll}>
		{checkingAll ? 'Checking…' : 'Check all'}
	</Button>
	<Button size="sm" data-testid="mcp-add-server" onclick={() => (adding = true)}>Add server…</Button>
</div>

<div class="pane" data-testid="mcp-page">
	{#if servers.error}
		<p class="bad" role="alert" data-testid="mcp-servers-error">{servers.error}</p>
	{/if}

	<div class="line">
		<span class="hint" data-testid="mcp-servers-summary">{summary}</span>
		{#if servers.warnings.length > 0}
			<span class="hint" data-testid="mcp-servers-warnings">{servers.warnings.join(' · ')}</span>
		{/if}
	</div>

	<div class="split">
		{#if !servers.loading && servers.items.length === 0 && !servers.error}
			<EmptyState
				title="No MCP servers found"
				hint="Atlas looked in the Claude Code, Codex, Cursor, Gemini CLI and Windsurf configs and the installed plugin caches."
			>
				<Button size="sm" onclick={() => (adding = true)}>Add server…</Button>
			</EmptyState>
		{:else}
			<ServerTable
				id="mcp-servers"
				{rows}
				checks={servers.checks}
				loading={servers.loading}
				selectedId={servers.openId}
				busyId={servers.busyId}
				emptyText="No server from this agent."
				onopen={(row) => openServer(row.id)}
				oncheck={runCheck}
				ontoggle={toggle}
				onremove={(row) => (removing = row)}
			/>
		{/if}

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
					{#if open?.is_atlas}
						<AtlasServerDetail {section} />
					{/if}
				{/snippet}
			</ServerDetail>
		{/if}
	</div>
</div>

<AddServerDialog
	open={adding}
	projectId={null}
	onclose={() => (adding = false)}
	onadd={addServer}
/>

<RemoveServerDialog server={removing} onclose={() => (removing = null)} onremove={remove} />

<style>
	.title-row {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 28px;
		flex: 0 0 28px;
	}

	.title {
		font-size: 15px;
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.pane {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.line {
		display: flex;
		align-items: center;
		gap: 12px;
	}

	.split {
		flex: 1;
		min-height: 0;
		display: flex;
		gap: 12px;
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
