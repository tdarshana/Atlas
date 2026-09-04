<script lang="ts">
	// The open MCP server, docked at the right of the table the way the skills detail is
	// docked beside its list: where the entry came from, how it is started, which secret
	// keys it reads, and what the last Check found. The Atlas row has content of its own
	// (its transports, tools, resources, prompts and clients), so the caller passes that
	// in as `atlas` and this panel keeps only the frame around it.
	import type { Snippet } from 'svelte';
	import { Badge, Button, IconButton } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { DETAIL_MAX, DETAIL_MIN, sourceLabel, toggleReason } from '$lib/mcp-servers';
	import { copyText } from '$lib/shell';
	import type { McpCheckResult, McpServerEntry } from '$lib/types';
	import ResizeBar from '$lib/ui/ResizeBar.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		server: McpServerEntry;
		check: McpCheckResult | null;
		width: number;
		busy?: boolean;
		onclose: () => void;
		onresize: (width: number) => void;
		oncheck: (row: McpServerEntry) => void;
		ontoggle: (row: McpServerEntry, enabled: boolean) => void;
		onremove: (row: McpServerEntry) => void;
		/** What the Atlas row shows instead of the generic body. */
		atlas?: Snippet;
	}

	let {
		server,
		check,
		width,
		busy = false,
		onclose,
		onresize,
		oncheck,
		ontoggle,
		onremove,
		atlas
	}: Props = $props();

	let node = $state<HTMLElement>();
	let copied = $state(false);
	let copyTimer: ReturnType<typeof setTimeout> | null = null;

	const secretKeys = $derived(
		server.transport.kind === 'stdio' ? server.transport.env_keys : server.transport.header_keys
	);
	const secretLabel = $derived(server.transport.kind === 'stdio' ? 'Environment' : 'Headers');

	async function copyPath(): Promise<void> {
		if (!server.file) return;
		try {
			await copyText(server.file);
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
</script>

<aside
	bind:this={node}
	class="detail"
	style="--detail-w:{width}px"
	aria-label="MCP server detail"
	data-testid="mcp-server-detail"
>
	<header>
		<span class="name" data-testid="mcp-server-detail-name">{server.name}</span>
		<Badge variant="outline" title={server.plugin ?? undefined} data-testid="mcp-server-detail-source">
			{sourceLabel(server.source)}
		</Badge>
		<Badge data-testid="mcp-server-detail-scope">{server.scope}</Badge>
		<span class="spacer"></span>
		<IconButton
			icon="x"
			label="Close MCP server detail"
			onclick={onclose}
			data-testid="mcp-server-detail-close"
		/>
	</header>

	<div class="body">
		<div class="actions">
			<Button size="sm" disabled={busy} data-testid="mcp-server-detail-check" onclick={() => oncheck(server)}>
				{busy ? 'Checking…' : 'Check'}
			</Button>
			{#if server.can_toggle}
				<Button
					size="sm"
					variant="ghost"
					disabled={busy}
					data-testid="mcp-server-detail-toggle"
					onclick={() => ontoggle(server, !server.enabled)}
				>
					{server.enabled ? 'Disable' : 'Enable'}
				</Button>
			{:else}
				<span class="hint" data-testid="mcp-server-detail-no-toggle">{toggleReason(server)}</span>
			{/if}
			<span class="spacer"></span>
			{#if server.can_remove}
				<Button
					size="sm"
					variant="ghost"
					data-testid="mcp-server-detail-remove"
					onclick={() => onremove(server)}
				>
					Remove…
				</Button>
			{/if}
		</div>

		{#if server.file}
			<div class="path-row">
				<span class="mono path" title={server.file}>{server.file}</span>
				<Button size="sm" variant="ghost" data-testid="mcp-server-detail-copy-path" onclick={copyPath}>
					{copied ? 'Copied' : 'Copy path'}
				</Button>
			</div>
		{/if}

		{#if atlas}
			{@render atlas()}
		{:else}
			<div class="group" data-testid="mcp-server-detail-transport">
				<span class="group-heading">Transport</span>
				{#if server.transport.kind === 'stdio'}
					<div class="reading">
						<span class="label">Command</span>
						<span class="mono value">{server.transport.command}</span>
					</div>
					{#if server.transport.args.length > 0}
						<div class="reading">
							<span class="label">Arguments</span>
							<span class="mono value">{server.transport.args.join(' ')}</span>
						</div>
					{/if}
				{:else}
					<div class="reading">
						<span class="label">URL</span>
						<span class="mono value">{server.transport.url}</span>
					</div>
				{/if}
			</div>

			{#if secretKeys.length > 0}
				<div class="group" data-testid="mcp-server-detail-secrets">
					<span class="group-heading">{secretLabel}</span>
					{#each secretKeys as key (key)}
						<div class="reading">
							<span class="mono label wide">{key}</span>
							<span class="mono value masked">••••••••</span>
						</div>
					{/each}
					<span class="hint">
						Values stay in the agent's own config file; Atlas only ever shows the key names.
					</span>
				</div>
			{/if}

			<div class="group" data-testid="mcp-server-detail-check-result">
				<span class="group-heading">Last check</span>
				{#if !check}
					<span class="hint">Not checked yet. Check starts this server and asks for its tools.</span>
				{:else if !check.ok}
					<p class="bad" role="alert">{check.error ?? 'The check failed.'}</p>
					<span class="hint">Took {check.elapsed_ms} ms.</span>
				{:else}
					<div class="reading">
						<span class="label">Server</span>
						<span class="mono value">
							{check.server_name ?? server.name}{check.server_version
								? ` ${check.server_version}`
								: ''}
						</span>
					</div>
					{#if check.protocol_version}
						<div class="reading">
							<span class="label">Protocol</span>
							<span class="mono value">{check.protocol_version}</span>
						</div>
					{/if}
					<span class="hint">
						{check.tools.length}
						{check.tools.length === 1 ? 'tool' : 'tools'} in {check.elapsed_ms} ms
					</span>
					<div class="tools">
						{#each check.tools as tool (tool.name)}
							<div class="tool-row">
								<span class="mono value">{tool.name}</span>
								<!-- A server may list a tool with no description; the row is then just
								     the name, rather than an empty line under it. -->
								{#if tool.description}<span class="hint">{tool.description}</span>{/if}
							</div>
						{/each}
					</div>
				{/if}
			</div>
		{/if}
	</div>

	<ResizeBar
		label="Resize MCP server detail"
		value={width}
		min={DETAIL_MIN}
		max={DETAIL_MAX}
		side="left"
		onlive={(w) => node?.style.setProperty('--detail-w', `${w}px`)}
		{onresize}
		testid="mcp-server-detail-resize"
	/>
</aside>

<style>
	.detail {
		position: relative;
		width: var(--detail-w, 380px);
		flex: 0 0 var(--detail-w, 380px);
		display: flex;
		flex-direction: column;
		min-height: 0;
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		background: var(--bg-raised);
	}

	header {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 8px 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.name {
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.spacer {
		flex: 1;
	}

	.body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 12px;
	}

	.actions {
		display: flex;
		align-items: center;
		gap: 6px;
		flex-wrap: wrap;
	}

	.path-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.path {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text-tertiary);
	}

	.group {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.reading {
		display: flex;
		align-items: baseline;
		gap: 12px;
		color: var(--text-secondary);
	}

	.label {
		width: 80px;
		flex: 0 0 80px;
	}

	.label.wide {
		width: 160px;
		flex: 0 0 160px;
	}

	.value {
		flex: 1;
		min-width: 0;
		color: var(--text-primary);
		word-break: break-all;
	}

	.masked {
		color: var(--text-tertiary);
	}

	.tools {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.tool-row {
		display: flex;
		flex-direction: column;
	}

	.tool-row .hint {
		white-space: normal;
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
