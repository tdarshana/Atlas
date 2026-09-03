<script lang="ts">
	// The Memories tab (frame 02.1): this project's own memories, filtered by kind chips
	// and a state select, with a "Remember…" dialog that writes a project-scoped memory.
	import { Badge, Button, Icon, Input, Select, Table, type TableColumn } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage, errorLogPath } from '$lib/errors';
	import { plural, relativeAge } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import { MEMORY_KINDS } from '$lib/stores/memories.svelte';
	import { project, setHeaderActions } from '$lib/stores/project.svelte';
	import type { Memory, MemoryKind } from '$lib/types';
	import RememberDialog from '$lib/components/project/RememberDialog.svelte';
	import {
		MEMORY_STATE_OPTIONS,
		memoryStatusesFor,
		passesKindFilter,
		toggleMemoryKind,
		type MemoryStateFilter
	} from '$lib/components/project/memories';
	import ErrorState from '$lib/ui/ErrorState.svelte';

	const SEARCH_DEBOUNCE_MS = 300;
	const SEARCH_LIMIT = 50;

	let query = $state('');
	let kinds = $state<MemoryKind[]>([]);
	// Plain string, not `MemoryStateFilter`: `Select`'s `bind:value` is a string, and the
	// value can only ever be one of `MEMORY_STATE_OPTIONS` anyway.
	let stateFilter = $state('active');
	let all = $state<Memory[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let logPath = $state<string | null>(null);
	let remembering = $state(false);

	const name = $derived(project.current?.name ?? 'Project');
	const hits = $derived(all.filter((m) => passesKindFilter(m.kind, kinds)));

	const columns: TableColumn<Memory>[] = [
		{ key: 'kind', label: 'Kind', width: '110px' },
		{ key: 'text', label: 'Text', mono: true },
		{ key: 'tags', label: 'Tags', width: '160px' },
		{ key: 'source', label: 'Source', width: '140px', mono: true },
		{ key: 'age', label: 'Age', width: '80px', align: 'right', sortable: true }
	];

	function source(m: Memory): string {
		return [m.source_agent, m.source_tool].filter(Boolean).join(' · ') || '-';
	}

	let generation = 0;

	async function load(): Promise<void> {
		const id = project.current?.id;
		if (!id) return;
		const g = ++generation;
		loading = true;
		try {
			const q = query.trim();
			let rows: Memory[];
			if (q) {
				const results = await api().search({ query: q, limit: SEARCH_LIMIT, project_id: id });
				rows = results.map((r) => r.memory);
			} else {
				const lists = await Promise.all(
					memoryStatusesFor(stateFilter as MemoryStateFilter).map((s) => api().listMemories(s, id))
				);
				rows = lists.flat();
			}
			if (g !== generation) return;
			all = rows;
			error = null;
			logPath = null;
		} catch (e) {
			if (g !== generation) return;
			all = [];
			error = errorMessage(e);
			logPath = errorLogPath(e);
		} finally {
			if (g === generation) loading = false;
		}
	}

	let timer: ReturnType<typeof setTimeout> | null = null;
	function scheduleLoad(delayMs = SEARCH_DEBOUNCE_MS): void {
		if (timer !== null) clearTimeout(timer);
		timer = setTimeout(() => {
			timer = null;
			void load();
		}, delayMs);
	}

	$effect(() => {
		const id = project.current?.id;
		if (id) void load();
	});

	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});

	$effect(() => {
		setStatusItems({ right: [{ text: `${name} · ${plural(hits.length, 'memory', 'memories')}` }] });
	});
</script>

{#snippet headerActions()}
	<Button variant="primary" data-testid="memories-remember" onclick={() => (remembering = true)}>
		Remember…
	</Button>
{/snippet}

<div class="filters">
	<div class="search">
		<Input
			bind:value={query}
			data-testid="memories-search"
			placeholder="Search project memories…"
			aria-label="Search project memories"
			oninput={() => scheduleLoad()}
		/>
	</div>
	<div class="chips">
		{#each MEMORY_KINDS as kind (kind)}
			<Badge
				tone={kinds.includes(kind) ? 'accent' : 'neutral'}
				class="chip"
				role="button"
				tabindex={0}
				data-testid="memories-kind-{kind}"
				onclick={() => (kinds = toggleMemoryKind(kinds, kind))}
				onkeydown={(e: KeyboardEvent) => {
					if (e.key === 'Enter' || e.key === ' ') {
						e.preventDefault();
						kinds = toggleMemoryKind(kinds, kind);
					}
				}}
			>
				{kind}
			</Badge>
		{/each}
	</div>
	<span class="spacer"></span>
	<div class="state">
		<Select
			bind:value={stateFilter}
			options={MEMORY_STATE_OPTIONS}
			aria-label="State"
			data-testid="memories-state"
			onchange={() => load()}
		/>
	</div>
</div>

{#if error}
	<ErrorState message={error} logPath={logPath ?? undefined}>
		<Button variant="primary" onclick={() => load()}>Retry</Button>
	</ErrorState>
{:else}
	<Table
		id="project-memories"
		{columns}
		rows={hits}
		rowKey={(m: Memory) => m.id}
	>
		{#snippet cell(m: Memory, column: TableColumn<Memory>)}
			{#if column.key === 'kind'}
				<Badge>{m.kind}</Badge>
			{:else if column.key === 'text'}
				<span class="text">{m.text}</span>
			{:else if column.key === 'tags'}
				<span class="tags">
					{#each m.tags as tag (tag)}<Badge tone="accent" mono>{tag}</Badge>{:else}<span
							class="muted">-</span
						>{/each}
				</span>
			{:else if column.key === 'source'}
				{source(m)}
			{:else}
				{relativeAge(m.created_at)}
			{/if}
		{/snippet}
		{#snippet empty()}
			<div class="empty">
				<Icon name="info" size={20} color="var(--text-tertiary)" />
				<span>{loading ? 'Loading…' : 'No memories match this filter.'}</span>
			</div>
		{/snippet}
	</Table>
	<p class="footnote">
		Showing memories tagged <span class="mono">{name}</span> or sourced from this root. Global memories
		are in Memories.
	</p>
{/if}

<RememberDialog
	open={remembering}
	projectId={project.current?.id ?? ''}
	onclose={() => (remembering = false)}
	onsaved={() => load()}
/>

<style>
	.filters {
		display: flex;
		align-items: center;
		gap: 8px;
		flex: 0 0 auto;
		margin-bottom: 12px;
	}

	.search {
		width: 360px;
	}

	.chips {
		display: flex;
		gap: 4px;
	}

	:global(.chip) {
		cursor: pointer;
	}

	.spacer {
		flex: 1;
	}

	.state {
		width: 140px;
	}

	.text {
		display: block;
		max-width: 60ch;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.tags {
		display: inline-flex;
		flex-wrap: wrap;
		gap: 4px;
	}

	.muted {
		color: var(--text-secondary);
	}

	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
		color: var(--text-tertiary);
	}

	.footnote {
		margin: 8px 0 0;
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
