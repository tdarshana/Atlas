<script lang="ts">
	// Memories: search or list, filtered by scope and kind, with a detail panel
	// whose Forget supersedes the memory and drops its row.
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import {
		MEMORY_KINDS,
		forgetMemory,
		cancelLoad,
		loadMemories,
		memories,
		scheduleLoad,
		toggleKind,
		type ScopeFilter
	} from '$lib/stores/memories.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import type { MemoryKind, RecallHit } from '$lib/types';
	import Badge from '$lib/ui/Badge.svelte';
	import Button from '$lib/ui/Button.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Input from '$lib/ui/Input.svelte';
	import Select from '$lib/ui/Select.svelte';
	import Table from '$lib/ui/Table.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	const scopeOptions = [
		{ value: 'all', label: 'All' },
		{ value: 'global', label: 'Global' },
		{ value: 'project', label: 'Project' }
	];

	const projectOptions = $derived([
		{ value: '', label: 'Any project' },
		...projects.items.map((p) => ({ value: p.id, label: p.name }))
	]);

	const columns = $derived([
		...(memories.scored ? [{ key: 'score', label: 'Score', width: '70px' }] : []),
		{ key: 'kind', label: 'Kind', width: '110px' },
		{ key: 'text', label: 'Text' },
		{ key: 'tags', label: 'Tags', width: '160px' },
		{ key: 'source', label: 'Source', width: '140px' },
		{ key: 'age', label: 'Age', width: '80px', align: 'right' as const }
	]);

	const selected = $derived(memories.selected);
	const projectName = $derived(
		selected?.project_id
			? (projects.items.find((p) => p.id === selected.project_id)?.name ?? selected.project_id)
			: null
	);

	let confirming = $state(false);
	let reason = $state('');
	let forgetting = $state(false);

	function onScope(value: string) {
		memories.scope = value as ScopeFilter;
		if (memories.scope !== 'project') memories.projectId = '';
		scheduleLoad(0);
	}

	function source(hit: RecallHit): string {
		const { source_agent, source_tool } = hit.memory;
		return [source_agent, source_tool].filter(Boolean).join(' · ') || '-';
	}

	async function confirmForget() {
		if (!selected) return;
		forgetting = true;
		try {
			await forgetMemory(selected.id, reason);
			push('success', 'Memory forgotten');
			confirming = false;
			reason = '';
		} catch (e) {
			// The daemon's `error` string is the whole explanation; show it verbatim.
			push('error', errorMessage(e));
		} finally {
			forgetting = false;
		}
	}

	onMount(() => {
		void loadProjects();
		void loadMemories();
		// `?id=<uuid>` (from the command palette, or a link elsewhere) selects that
		// memory in the detail panel even when it falls outside the current filters.
		const id = page.url.searchParams.get('id');
		if (id) {
			api()
				.getMemory(id)
				.then((m) => (memories.selected = m))
				.catch(() => {});
		}
		// A pending debounce would fire a request for a screen that is gone.
		return cancelLoad;
	});

	$effect(() => {
		setStatusItems({ right: [{ text: `${memories.hits.length} memories` }] });
	});
</script>

<div class="head">
	<h1>Memories</h1>
</div>

<div class="filters">
	<div class="search">
		<Input
			bind:value={memories.query}
			data-testid="memories-search"
			placeholder="Search memories…"
			aria-label="Search memories"
			oninput={() => scheduleLoad()}
		/>
	</div>

	<div class="scope">
		<Select
			value={memories.scope}
			options={scopeOptions}
			aria-label="Scope"
			onchange={(e) => onScope(e.currentTarget.value)}
		/>
	</div>

	{#if memories.scope === 'project'}
		<div class="scope">
			<Select
				bind:value={memories.projectId}
				options={projectOptions}
				aria-label="Project"
				data-testid="memories-project"
				onchange={() => scheduleLoad(0)}
			/>
		</div>
	{/if}
</div>

<div class="chips">
	{#each MEMORY_KINDS as kind (kind)}
		<button
			type="button"
			class="chip"
			class:on={memories.kinds.includes(kind)}
			aria-pressed={memories.kinds.includes(kind)}
			data-testid="kind-{kind}"
			onclick={() => toggleKind(kind)}
		>
			{kind}
		</button>
	{/each}
	{#if memories.kinds.length > 0}
		<button type="button" class="chip clear" onclick={() => { memories.kinds = []; scheduleLoad(0); }}>
			clear
		</button>
	{/if}
</div>

<div class="split" class:detail={!!selected}>
	<div>
		{#if memories.error}
			<ErrorState message={memories.error} logPath={memories.errorLogPath ?? undefined}>
				<Button variant="primary" onclick={() => loadMemories()}>Retry</Button>
			</ErrorState>
		{:else}
			<Table
				{columns}
				rows={memories.hits}
				data-testid="memories-table"
				rowKey={(hit: RecallHit) => hit.memory.id}
				onrowclick={(hit: RecallHit) => (memories.selected = hit.memory)}
			>
				{#snippet cell(hit: RecallHit, key: string)}
					{#if key === 'score'}
						<span class="muted">{hit.score.toFixed(2)}</span>
					{:else if key === 'kind'}
						<Badge>{hit.memory.kind}</Badge>
					{:else if key === 'text'}
						<span class="text">{hit.memory.text}</span>
					{:else if key === 'tags'}
						{#each hit.memory.tags as tag (tag)}<Badge tone="accent">{tag}</Badge>{:else}
							<span class="muted">-</span>
						{/each}
					{:else if key === 'source'}
						<span class="muted">{source(hit)}</span>
					{:else}
						<span class="muted">{relativeAge(hit.memory.created_at)}</span>
					{/if}
				{/snippet}
				{#snippet empty()}
					{#if memories.loading}
						<EmptyState title="Loading…" />
					{:else if memories.query.trim()}
						<EmptyState
							title="No matches"
							hint="Nothing active matches that query with the current filters."
						/>
					{:else}
						<EmptyState
							title="No memories yet"
							hint="Agents write memories through the MCP tools, or add one with `atlas remember`."
						/>
					{/if}
				{/snippet}
			</Table>
		{/if}
	</div>

	{#if selected}
		<aside class="panel" data-testid="memory-detail">
			<header>
				<Badge>{selected.kind}</Badge>
				<button
					type="button"
					class="x"
					aria-label="Close detail"
					onclick={() => (memories.selected = null)}>×</button
				>
			</header>

			<p class="body">{selected.text}</p>

			<dl>
				<dt>Scope</dt>
				<dd>{selected.scope}{projectName ? ` · ${projectName}` : ''}</dd>
				<dt>Tags</dt>
				<dd>
					{#each selected.tags as tag (tag)}<Badge tone="accent">{tag}</Badge>{:else}-{/each}
				</dd>
				<dt>Source</dt>
				<dd>{[selected.source_agent, selected.source_tool].filter(Boolean).join(' · ') || '-'}</dd>
				<dt>Confidence</dt>
				<dd>{selected.confidence.toFixed(2)}</dd>
				<dt>Status</dt>
				<dd>{selected.status}</dd>
				<dt>Created</dt>
				<dd>{new Date(selected.created_at).toLocaleString()}</dd>
				<dt>Id</dt>
				<dd><code>{selected.id}</code></dd>
			</dl>

			<Button variant="danger" data-testid="memory-forget" onclick={() => (confirming = true)}>
				Forget
			</Button>
		</aside>
	{/if}
</div>

<Dialog
	open={confirming}
	title="Forget this memory?"
	onclose={() => {
		confirming = false;
		reason = '';
	}}
>
	<p class="muted">
		Nothing is deleted; the memory is superseded and stops appearing in recall.
	</p>
	<label class="label" for="forget-reason">Reason (optional)</label>
	<Textarea id="forget-reason" bind:value={reason} rows={3} placeholder="Why is this no longer true?" />

	{#snippet footer()}
		<Button onclick={() => { confirming = false; reason = ''; }}>Cancel</Button>
		<Button
			variant="danger"
			data-testid="memory-forget-confirm"
			disabled={forgetting}
			onclick={confirmForget}
		>
			{forgetting ? 'Forgetting…' : 'Forget'}
		</Button>
	{/snippet}
</Dialog>

<style>
	.head {
		display: flex;
		align-items: center;
		height: 28px;
		flex: 0 0 28px;
		margin-bottom: var(--space-4);
	}

	.head h1 {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
	}

	.filters {
		display: flex;
		gap: var(--space-3);
		margin-bottom: var(--space-3);
	}

	.search {
		flex: 1;
	}

	.scope {
		width: 180px;
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-2);
		margin-bottom: var(--space-4);
	}

	.chip {
		padding: 2px 10px;
		border: 1px solid var(--border-default);
		border-radius: 999px;
		background: var(--bg-raised);
		color: var(--text-secondary);
		font: inherit;
		font-size: 13px;
		cursor: pointer;
	}

	.chip.on {
		border-color: transparent;
		background: var(--accent-muted);
		color: var(--accent);
	}

	.chip.clear {
		border-style: dashed;
	}

	.split {
		display: grid;
		gap: var(--space-4);
		align-items: start;
	}

	.split.detail {
		grid-template-columns: 1fr 320px;
	}

	.panel {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		padding: var(--space-4);
		border: 1px solid var(--border-default);
		border-radius: var(--radius-md);
		background: var(--bg-raised);
	}

	.panel header {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	.x {
		border: none;
		background: none;
		color: var(--text-secondary);
		font-size: 20px;
		line-height: 1;
		cursor: pointer;
	}

	.body {
		margin: 0;
		overflow-wrap: anywhere;
	}

	.text {
		display: block;
		max-width: 60ch;
		overflow-wrap: anywhere;
	}

	.muted {
		color: var(--text-secondary);
	}

	dl {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: var(--space-1) var(--space-3);
		margin: 0;
		font-size: 13px;
	}

	dt {
		color: var(--text-secondary);
	}

	dd {
		margin: 0;
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-1);
		overflow-wrap: anywhere;
	}

	.label {
		display: block;
		margin: var(--space-3) 0 var(--space-1);
		font-size: 13px;
		color: var(--text-secondary);
	}
</style>
