<script lang="ts">
	// Memories (frame 05): search or list the global memory set, narrowed by a scope
	// select and the kind chips, with the side panel carrying the facets. The palette
	// links here with `?id=` to point at one memory and `?remember=1` to write one.
	import { onMount, untrack } from 'svelte';
	import { replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import { Badge, Button, Input, Select, Table, type TableColumn } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { NOTHING, plural, relativeAge } from '$lib/format';
	import { copyText, setStatusItems } from '$lib/shell';
	import { scopeOptions, scopeSelection, scopeValue } from '$lib/components/memory-scope';
	import RememberDialog from '$lib/components/RememberDialog.svelte';
	import { push } from '$lib/platform/toasts.svelte';
	import { decide } from '$lib/stores/review.svelte';
	import {
		MEMORY_KINDS,
		cancelLoad,
		followMemoryChanges,
		forgetMemory,
		loadMemories,
		loadMore,
		memories,
		scheduleLoad,
		toggleKind
	} from '$lib/stores/memories.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import type { MemoryKind, RecallHit } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';

	const columns: TableColumn<RecallHit>[] = [
		{ key: 'kind', label: 'Kind', width: '110px', sortable: true, sort: (a, b) => a.memory.kind.localeCompare(b.memory.kind) },
		{ key: 'text', label: 'Text', mono: true },
		{ key: 'tags', label: 'Tags', width: '200px', mono: true },
		{ key: 'source', label: 'Source', width: '120px', mono: true },
		{
			key: 'age',
			label: 'Age',
			width: '80px',
			align: 'right',
			mono: true,
			sortable: true,
			sort: (a, b) => a.memory.created_at.localeCompare(b.memory.created_at)
		}
	];

	const options = $derived(scopeOptions(projects.items));
	const scope = $derived(scopeValue(memories.scope, memories.projectId));
	const selected = $derived(memories.selected);
	const projectName = $derived(
		selected?.project_id
			? (projects.items.find((p) => p.id === selected.project_id)?.name ?? selected.project_id)
			: null
	);

	let remembering = $state(false);
	let deciding = $state(false);

	/** Accept or reject a pending memory from its card, the way Review does. */
	async function decideHere(status: 'active' | 'rejected'): Promise<void> {
		const target = memories.selected;
		if (!target) return;
		deciding = true;
		try {
			await decide(target.id, status);
			memories.selected = null;
			await loadMemories();
			push('success', status === 'active' ? 'Memory accepted' : 'Memory rejected');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			deciding = false;
		}
	}
	let confirming = $state(false);
	let reason = $state('');
	let forgetting = $state(false);
	let tableEl = $state<HTMLDivElement>();

	/** Insight reads as information; a todo is pending work. The rest carry no tone. */
	function kindTone(kind: MemoryKind): 'accent' | 'info' | 'warning' {
		if (kind === 'insight') return 'info';
		if (kind === 'todo') return 'warning';
		return 'accent';
	}

	function onScope(value: string): void {
		const chosen = scopeSelection(value);
		memories.scope = chosen.scope;
		memories.projectId = chosen.projectId;
		scheduleLoad(0);
	}

	function source(hit: RecallHit): string {
		const { source_agent, source_tool } = hit.memory;
		return [source_agent, source_tool].filter(Boolean).join(' · ') || NOTHING;
	}

	let textCopied = $state(false);

	async function copyMemoryText(): Promise<void> {
		if (!selected) return;
		try {
			await copyText(selected.text);
			textCopied = true;
			setTimeout(() => (textCopied = false), 1500);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	async function confirmForget(): Promise<void> {
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
		// A pending debounce would fire a request for a screen that is gone.
		const unfollow = followMemoryChanges();
		return () => {
			cancelLoad();
			unfollow();
		};
	});

	// `?id=<uuid>` (from the command palette, or a link elsewhere) selects that memory even
	// when it falls outside the current filters. It reads the URL rather than running once,
	// so a second palette hit while this page is already open re-points the selection.
	$effect(() => {
		const id = page.url.searchParams.get('id');
		if (!id) return;
		untrack(() => {
			api()
				.getMemory(id)
				.then((m) => (memories.selected = m))
				.catch(() => {});
		});
	});

	// `?remember=1` opens the write dialog straight from the palette.
	$effect(() => {
		if (page.url.searchParams.get('remember') === '1') untrack(() => (remembering = true));
	});

	/**
	 * Closing the dialog drops `?remember=1` with it, so the next thing that touches the
	 * URL does not reopen a dialog the user has just dismissed.
	 */
	function closeRemember(): void {
		remembering = false;
		if (!page.url.searchParams.has('remember')) return;
		const url = new URL(page.url);
		url.searchParams.delete('remember');
		replaceState(url, page.state);
	}

	// The selected row may be far down a long list, so bring it into view once it is
	// rendered. jsdom and older webviews have no `scrollIntoView`; the row is still marked.
	$effect(() => {
		const id = memories.selected?.id;
		if (!id || !tableEl) return;
		const row = tableEl.querySelector('.row.selected');
		if (row && typeof row.scrollIntoView === 'function') {
			row.scrollIntoView({ behavior: 'instant', block: 'nearest' });
		}
	});

	// With a further page to fetch the count shown is the loaded part; the facets
	// total (the same scope, every kind) says how many there are altogether.
	$effect(() => {
		const shown = plural(memories.hits.length, 'memory', 'memories');
		const text = memories.hasMore ? `${shown} of ${memories.facets.total}` : shown;
		setStatusItems({ right: [{ text }] });
	});
</script>

<div class="title-row">
	<span class="title">Memories</span>
	<span class="spacer"></span>
	<Button variant="primary" data-testid="memories-remember" onclick={() => (remembering = true)}>Remember…</Button>
	<div class="search">
		<Input
			bind:value={memories.query}
			icon="search"
			data-testid="memories-search"
			placeholder="Search memories…"
			aria-label="Search memories"
			oninput={() => scheduleLoad()}
		/>
	</div>
	<div class="scope">
		<Select
			value={scope}
			{options}
			aria-label="Scope"
			data-testid="memories-scope"
			onchange={(e) => onScope(e.currentTarget.value)}
		/>
	</div>
</div>

<div class="chips">
	{#each MEMORY_KINDS as kind (kind)}
		{@const on = memories.kinds.includes(kind)}
		<button
			type="button"
			class="chip"
			aria-pressed={on}
			data-testid="kind-{kind}"
			onclick={() => toggleKind(kind)}
		>
			<Badge tone={on ? kindTone(kind) : 'neutral'} variant={on ? 'soft' : 'outline'}>
				{kind}
			</Badge>
		</button>
	{/each}
</div>

<div class="split" class:detail={!!selected}>
	<div class="list" bind:this={tableEl} data-testid="memories-table">
		{#if memories.error}
			<ErrorState message={memories.error} logPath={memories.errorLogPath ?? undefined}>
				<Button variant="primary" onclick={() => loadMemories()}>Retry</Button>
			</ErrorState>
		{:else}
			<Table
				id="memories"
				{columns}
				rows={memories.hits}
				rowKey={(hit: RecallHit) => hit.memory.id}
				selectedKey={selected?.id ?? null}
				onRowClick={(hit: RecallHit) => (memories.selected = hit.memory)}
				defaultSort={{ key: 'age', dir: 'desc' }}
			>
				{#snippet cell(hit: RecallHit, column: TableColumn<RecallHit>)}
					{#if column.key === 'kind'}
						<Badge tone={kindTone(hit.memory.kind)}>{hit.memory.kind}</Badge>
					{:else if column.key === 'text'}
						{hit.memory.text}
					{:else if column.key === 'tags'}
						<span class="tags">
							{#each hit.memory.tags as tag (tag)}<Badge tone="accent" mono>{tag}</Badge>{:else}
								<span class="muted">{NOTHING}</span>
							{/each}
						</span>
					{:else if column.key === 'source'}
						<span class="muted">{source(hit)}</span>
					{:else}
						<span class="muted">{relativeAge(hit.memory.created_at)}</span>
					{/if}
				{/snippet}
				{#snippet empty()}
					<div class="empty">
						{#if memories.loading}
							<span class="empty-title">Loading…</span>
						{:else if memories.query.trim()}
							<span class="empty-title">No matches</span>
							<span class="empty-hint">
								Nothing active matches that query with the current filters.
							</span>
						{:else}
							<span class="empty-title">No memories yet</span>
							<span class="empty-hint">
								Agents write memories through the MCP tools, or add one with atlas remember.
							</span>
						{/if}
					</div>
				{/snippet}
			</Table>
			{#if memories.hasMore}
				<div class="more">
					<Button size="sm" onclick={() => loadMore()} disabled={memories.loading} data-testid="memories-more">
						Show more
					</Button>
				</div>
			{/if}
		{/if}
	</div>

	{#if selected}
		<aside class="card panel-detail" data-testid="memory-detail">
			<div class="detail-head">
				<Badge tone={kindTone(selected.kind)}>{selected.kind}</Badge>
				<span class="spacer"></span>
				<Button variant="ghost" size="sm" onclick={copyMemoryText}>
					{textCopied ? 'Copied' : 'Copy'}
				</Button>
				<Button variant="ghost" size="sm" onclick={() => (memories.selected = null)}>Close</Button>
			</div>

			<div class="detail-body">
				<p class="detail-text">{selected.text}</p>

				<dl>
					<dt>Scope</dt>
					<dd>{selected.scope}{projectName ? ` · ${projectName}` : ''}</dd>
					<dt>Tags</dt>
					<dd>
						{#each selected.tags as tag (tag)}<Badge tone="accent" mono>{tag}</Badge>{:else}
							{NOTHING}
						{/each}
					</dd>
					<dt>Source</dt>
					<dd class="mono">
						{[selected.source_agent, selected.source_tool].filter(Boolean).join(' · ') || NOTHING}
					</dd>
					<dt>Confidence</dt>
					<dd class="mono">{selected.confidence.toFixed(2)}</dd>
					<dt>Status</dt>
					<dd>{selected.status}</dd>
					<dt>Created</dt>
					<dd class="mono">{new Date(selected.created_at).toLocaleString()}</dd>
					<dt>Id</dt>
					<dd class="mono id">{selected.id}</dd>
				</dl>

				<div class="detail-actions">
					{#if selected.status === 'pending'}
						<Button variant="primary" data-testid="memory-accept" disabled={deciding} onclick={() => decideHere('active')}>Accept</Button>
						<Button data-testid="memory-reject" disabled={deciding} onclick={() => decideHere('rejected')}>Reject</Button>
					{/if}
					<Button variant="danger" data-testid="memory-forget" onclick={() => (confirming = true)}>
						Forget…
					</Button>
				</div>
			</div>
		</aside>
	{/if}
</div>

<RememberDialog
	open={remembering}
	projects={projects.items}
	onclose={closeRemember}
	onsaved={() => loadMemories()}
/>

<Dialog
	open={confirming}
	title="Forget this memory?"
	onclose={() => {
		confirming = false;
		reason = '';
	}}
>
	<p class="muted">Nothing is deleted; the memory is superseded and stops appearing in recall.</p>
	<label class="reason" for="forget-reason">Reason (optional)</label>
	<Textarea
		id="forget-reason"
		bind:value={reason}
		rows={3}
		placeholder="Why is this no longer true?"
	/>

	{#snippet footer()}
		<Button
			onclick={() => {
				confirming = false;
				reason = '';
			}}
		>
			Cancel
		</Button>
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

	.search {
		width: 300px;
	}

	.scope {
		width: 120px;
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
		flex: 0 0 auto;
	}

	/* The Badge carries the whole look; the button is only here so a chip is a control
	   a keyboard can reach and a screen reader reads as pressed or not. */
	.chip {
		padding: 0;
		border: 0;
		background: none;
		font: inherit;
		cursor: default;
	}

	.split {
		flex: 1;
		min-height: 0;
		display: grid;
		grid-template-columns: 1fr;
		gap: 12px;
	}

	.split.detail {
		grid-template-columns: 1fr 320px;
	}

	.list {
		min-width: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	.more {
		flex: 0 0 auto;
		display: flex;
		justify-content: center;
		padding: 8px 0;
	}

	.panel-detail {
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow: hidden;
	}

	.detail-head {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.detail-body {
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 10px;
		overflow-y: auto;
	}

	.detail-text {
		margin: 0;
		overflow-wrap: anywhere;
	}

	.tags {
		display: inline-flex;
		flex-wrap: wrap;
		gap: 4px;
	}

	.muted {
		color: var(--text-secondary);
	}

	.mono {
		font-family: var(--font-mono);
	}

	dl {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 4px 12px;
		margin: 0;
	}

	dt {
		color: var(--text-secondary);
	}

	dd {
		margin: 0;
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
		overflow-wrap: anywhere;
	}

	.id {
		font-size: 11px;
	}

	.reason {
		display: block;
		margin: 12px 0 4px;
		color: var(--text-secondary);
	}

	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 6px;
		padding: 28px 0;
	}

	.empty-title {
		font-weight: 600;
	}

	.empty-hint {
		color: var(--text-secondary);
	}
	.detail-actions {
		display: flex;
		gap: 8px;
		align-items: center;
	}
</style>
