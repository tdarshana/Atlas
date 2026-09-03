<script lang="ts">
	// Practices and workflows differ only in their kind, so both routes render this list
	// plus `DocEditor` against the store built for their kind. Practices is frame 07;
	// Workflows borrows the same list until Phase 9 gives it an editor of its own.

	import { onMount } from 'svelte';
	import { Badge, Button, Icon, Table, type TableColumn } from '$lib/ds';
	import { api, daemon } from '$lib/daemon.svelte';
	import { relativeAge } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import type { DocsStore } from '$lib/stores/docs.svelte';
	import type { Doc, Project } from '$lib/types';
	import DocEditor from './DocEditor.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';

	interface Props {
		store: DocsStore;
		/** Plural heading, e.g. "Practices". */
		title: string;
		/** Singular noun used in buttons and dialogs, e.g. "practice". */
		noun: string;
		hint: string;
	}

	let { store, title, noun, hint }: Props = $props();

	const columns: TableColumn<Doc>[] = [
		{ key: 'name', label: 'Name', width: '220px', sortable: true },
		{ key: 'tags', label: 'Tags' },
		{ key: 'project', label: 'Project', width: '160px' },
		{
			key: 'updated',
			label: 'Updated',
			width: '100px',
			align: 'right',
			mono: true,
			sortable: true,
			sort: (a, b) => a.updated_at.localeCompare(b.updated_at)
		}
	];

	let projects = $state<Project[]>([]);
	let open = $state(false);
	let editing = $state<Doc | null>(null);

	onMount(() => {
		void store.load();
		void api()
			.listProjects()
			// The list still works without projects; the select just has no entries.
			.then((p) => (projects = p))
			.catch(() => {});
	});

	$effect(() => {
		setStatusItems({ right: [{ text: `${store.state.list.length} ${title.toLowerCase()}` }] });
	});

	function projectName(id: string | null): string {
		if (id === null) return 'global';
		return projects.find((p) => p.id === id)?.name ?? id;
	}

	function openNew(): void {
		editing = null;
		open = true;
	}

	function openEdit(doc: Doc): void {
		editing = doc;
		open = true;
	}
</script>

<div class="title-row">
	<span class="title">{title}</span>
	<span class="spacer"></span>
	<Button variant="primary" data-testid="doc-new" onclick={openNew}>New {noun}</Button>
</div>

{#if store.state.error}
	<ErrorState message={store.state.error} logPath={daemon.logPath || undefined}>
		<Button variant="primary" onclick={() => store.load()}>Retry</Button>
	</ErrorState>
{:else}
	<div class="list" data-testid="docs-table">
		<Table
			id="docs-{store.kind}"
			{columns}
			rows={store.state.list}
			rowKey={(d: Doc) => d.id}
			onRowClick={openEdit}
			defaultSort={{ key: 'name', dir: 'asc' }}
		>
			{#snippet cell(doc: Doc, column: TableColumn<Doc>)}
				{#if column.key === 'name'}
					<span class="link">{doc.name}</span>
				{:else if column.key === 'tags'}
					<span class="tags">
						{#each doc.tags as tag (tag)}<Badge mono>{tag}</Badge>{/each}
					</span>
				{:else if column.key === 'project'}
					{#if doc.project_id === null}
						<Badge>global</Badge>
					{:else}
						<span class="muted">{projectName(doc.project_id)}</span>
					{/if}
				{:else}
					{relativeAge(doc.updated_at)}
				{/if}
			{/snippet}
			{#snippet empty()}
				<div class="empty">
					{#if store.state.loading}
						<Icon name="info" size={20} color="var(--text-tertiary)" />
						<span class="empty-title">Loading…</span>
					{:else}
						<span class="empty-title">No {title.toLowerCase()} yet</span>
						<span class="empty-hint">{hint}</span>
						<div class="empty-action">
							<Button variant="primary" onclick={openNew}>New {noun}</Button>
						</div>
					{/if}
				</div>
			{/snippet}
		</Table>
	</div>
{/if}

<DocEditor
	{open}
	{editing}
	{noun}
	{projects}
	onclose={() => (open = false)}
	onsave={(d) => store.save(d)}
	ondelete={(name) => store.remove(name)}
/>

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

	.list {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	.link {
		font-family: var(--font-mono);
		color: var(--accent);
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
		gap: 6px;
		padding: 28px 0;
	}

	.empty-title {
		font-weight: 600;
	}

	.empty-hint {
		color: var(--text-secondary);
	}

	.empty-action {
		margin-top: 4px;
	}
</style>
