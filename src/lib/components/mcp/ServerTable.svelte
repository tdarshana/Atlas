<script lang="ts">
	// The MCP servers table, drawn the same way in the global view and the project tab.
	// One row per server an agent is wired to, Atlas among them. The Enabled column is a
	// checkbox only where the agent has a switch of its own; everywhere else it is a
	// static badge carrying the reason. Status stays empty until the row has been
	// checked, because a check starts the user's own command.
	import { Badge, Checkbox, IconButton, Table, type TableColumn } from '$lib/ds';
	import { sourceLabel, toggleReason, transportSummary } from '$lib/mcp-servers';
	import type { McpCheckResult, McpServerEntry } from '$lib/types';

	interface Props {
		id: string;
		rows: McpServerEntry[];
		checks: Record<string, McpCheckResult>;
		selectedId?: string | null;
		loading?: boolean;
		emptyText?: string;
		/** The label over the Enabled column: `Enabled` globally, `Enabled here` per project. */
		enabledLabel?: string;
		onopen: (row: McpServerEntry) => void;
		ontoggle: (row: McpServerEntry, enabled: boolean) => void;
		oncheck: (row: McpServerEntry) => void;
		onremove: (row: McpServerEntry) => void;
		/** The id being checked or written right now, so its controls hold still. */
		busyId?: string | null;
	}

	let {
		id,
		rows,
		checks,
		selectedId = null,
		loading = false,
		emptyText = 'No MCP servers found.',
		enabledLabel = 'Enabled',
		onopen,
		ontoggle,
		oncheck,
		onremove,
		busyId = null
	}: Props = $props();

	let menuId = $state<string | null>(null);

	// Name, Transport and Status take whatever is left over and truncate, so the row fits
	// the width it is given rather than pushing a horizontal scrollbar under the table
	// when the detail panel is open. Everything else is a fixed, narrow track.
	const columns = $derived<TableColumn<McpServerEntry>[]>([
		{ key: 'name', label: 'Name', width: 'minmax(0, 2fr)', mono: true, sortable: true },
		{ key: 'source', label: 'Agent', width: '104px', sortable: true },
		{ key: 'scope', label: 'Scope', width: '68px', sortable: true },
		{ key: 'transport', label: 'Transport', width: 'minmax(0, 3fr)', mono: true },
		{ key: 'enabled', label: enabledLabel, width: '96px' },
		{ key: 'status', label: 'Status', width: 'minmax(0, 2fr)' },
		{ key: 'actions', label: '', width: '32px' }
	]);

	/** The whole transport, for the Transport cell's title: the command with its arguments,
	 * or the URL. */
	function transportTitle(row: McpServerEntry): string {
		return row.transport.kind === 'http'
			? row.transport.url
			: [row.transport.command, ...row.transport.args].join(' ');
	}

	/** What the Status cell says once a check has run: the tool count, or the reason it
	 * failed. Empty before the first check, because nothing has been started yet. */
	function statusText(row: McpServerEntry): string {
		const result = checks[row.id];
		if (!result) return '';
		if (!result.ok) return result.error ?? 'Check failed';
		const noun = result.tools.length === 1 ? 'tool' : 'tools';
		return `${result.tools.length} ${noun}`;
	}

	function toggleMenu(row: McpServerEntry): void {
		menuId = menuId === row.id ? null : row.id;
	}

	function run(row: McpServerEntry, action: (row: McpServerEntry) => void): void {
		menuId = null;
		action(row);
	}
</script>

<svelte:window onclick={() => (menuId = null)} />

<div class="servers-table" data-testid="{id}-wrap">
	<Table
		{id}
		{columns}
		{rows}
		rowKey={(s: McpServerEntry) => s.id}
		selectedKey={selectedId}
		onRowClick={onopen}
	>
		{#snippet cell(row: McpServerEntry, column: TableColumn<McpServerEntry>)}
			{#if column.key === 'name'}
				<span class="name" class:off={!row.enabled} title={row.name}>{row.name}</span>
			{:else if column.key === 'source'}
				<Badge
					variant="outline"
					title={row.plugin ?? undefined}
					data-testid="mcp-server-source-{row.id}"
				>
					{sourceLabel(row.source)}
				</Badge>
			{:else if column.key === 'scope'}
				{row.scope}
			{:else if column.key === 'transport'}
				<!-- The title is the whole transport, since the cell shows a summary that
				     elides both the command and, when the column is narrow, itself. -->
				<span class="transport" title={transportTitle(row)}>{transportSummary(row.transport)}</span>
			{:else if column.key === 'enabled'}
				{#if row.can_toggle}
					<Checkbox
						checked={row.enabled}
						disabled={busyId === row.id}
						aria-label={`Enable ${row.name}`}
						data-testid="mcp-server-toggle-{row.id}"
						onclick={(e) => e.stopPropagation()}
						onchange={(e) => ontoggle(row, e.currentTarget.checked)}
					/>
				{:else}
					<Badge
						tone={row.enabled ? 'success' : 'neutral'}
						title={toggleReason(row)}
						data-testid="mcp-server-fixed-{row.id}"
					>
						{row.enabled ? 'on' : 'off'}
					</Badge>
				{/if}
			{:else if column.key === 'status'}
				<span
					class="status"
					class:bad={checks[row.id] && !checks[row.id].ok}
					title={checks[row.id] ? `Checked in ${checks[row.id].elapsed_ms} ms` : undefined}
					data-testid="mcp-server-status-{row.id}"
				>
					{busyId === row.id ? 'Checking…' : statusText(row)}
				</span>
			{:else if column.key === 'actions'}
				<span class="menu-wrap">
					<IconButton
						icon="ellipsis"
						size="sm"
						label={`Actions for ${row.name}`}
						data-testid="mcp-server-menu-{row.id}"
						onclick={(e) => {
							e.stopPropagation();
							toggleMenu(row);
						}}
					/>
					{#if menuId === row.id}
						<!-- svelte-ignore a11y_click_events_have_key_events -->
						<!-- svelte-ignore a11y_no_static_element_interactions -->
						<div class="menu" role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()}>
							<button
								type="button"
								role="menuitem"
								data-testid="mcp-server-check-{row.id}"
								onclick={() => run(row, oncheck)}
							>
								Check
							</button>
							{#if row.can_remove}
								<button
									type="button"
									role="menuitem"
									class="danger"
									data-testid="mcp-server-remove-{row.id}"
									onclick={() => run(row, onremove)}
								>
									Remove…
								</button>
							{/if}
						</div>
					{/if}
				</span>
			{/if}
		{/snippet}
		{#snippet empty()}
			<span class="hint" data-testid="{id}-empty">{loading ? 'Loading…' : emptyText}</span>
		{/snippet}
	</Table>
</div>

<style>
	/* The table sizes to its rows: no height of its own and no inner scroll region, so a
	   five-row group is five rows tall rather than a short box with the last row behind a
	   scrollbar. The list around it is what scrolls. The DS table body sets `overflow-y:
	   auto`, which also computes `overflow-x` to `auto` and is where the stray horizontal
	   scrollbar came from, so both are turned off here. */
	.servers-table {
		flex: 0 0 auto;
		display: flex;
		flex-direction: column;
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		overflow: hidden;
	}

	.servers-table :global(.body) {
		overflow: visible;
	}

	/* A server the agent has turned off is still listed, just visibly not in play. */
	.off {
		color: var(--text-tertiary);
	}

	.transport,
	.status {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.status {
		font-size: 11px;
		color: var(--text-secondary);
	}

	.status.bad {
		color: var(--danger-text);
	}

	.menu-wrap {
		position: relative;
		display: inline-flex;
	}

	.menu {
		position: absolute;
		top: 100%;
		right: 0;
		z-index: 20;
		min-width: 120px;
		display: flex;
		flex-direction: column;
		padding: 4px;
		border: 1px solid var(--border-default);
		border-radius: var(--radius-lg);
		background: var(--bg-overlay);
		box-shadow: var(--shadow-lg);
	}

	.menu button {
		appearance: none;
		border: 0;
		background: none;
		color: var(--text-primary);
		font: inherit;
		text-align: left;
		padding: 4px 8px;
		border-radius: 3px;
		cursor: pointer;
	}

	.menu button:hover {
		background: var(--bg-hover);
	}

	.menu button.danger {
		color: var(--danger-text);
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
