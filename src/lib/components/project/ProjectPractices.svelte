<script lang="ts">
	// The project's practices, shown on its Skills tab under the Practices strip: the
	// project list, then the global ones it inherits. Moved here from the former
	// Practices tab (2026-09-06) so one view holds every instruction agents follow.
	// The Practices tab (frame 02.3): this project's own practices, plus the global ones
	// it inherits. `listDocs('practice', projectId)` already answers with exactly that set
	// (see `docs.ts`'s `list`), so the two tables are one fetch split client-side.
	import { Badge, Button, Table, type TableColumn } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage, errorLogPath } from '$lib/errors';
	import { plural, relativeAge } from '$lib/format';
	import { project } from '$lib/stores/project.svelte';
	import type { Doc, NewDoc } from '$lib/types';
	import PracticeDialog from '$lib/components/project/PracticeDialog.svelte';
	import { splitPractices } from '$lib/components/project/practices';
	import ErrorState from '$lib/ui/ErrorState.svelte';

	let all = $state<Doc[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let logPath = $state<string | null>(null);

	let open = $state(false);
	let editing = $state<Doc | null>(null);

	const name = $derived(project.current?.name ?? 'Project');
	const id = $derived(project.current?.id ?? '');
	const split = $derived(splitPractices(all, id));

	const columns: TableColumn<Doc>[] = [
		{ key: 'name', label: 'Name', width: '220px', sortable: true },
		{ key: 'tags', label: 'Tags' },
		{ key: 'scope', label: 'Scope', width: '120px' },
		{ key: 'updated', label: 'Updated', width: '100px', align: 'right' }
	];

	async function load(): Promise<void> {
		if (!id) return;
		loading = true;
		try {
			all = await api().listDocs('practice', id);
			error = null;
			logPath = null;
		} catch (e) {
			all = [];
			error = errorMessage(e);
			logPath = errorLogPath(e);
		} finally {
			loading = false;
		}
	}

	function openNew(): void {
		editing = null;
		open = true;
	}

	function openEdit(doc: Doc): void {
		editing = doc;
		open = true;
	}

	async function save(d: NewDoc): Promise<Doc> {
		const saved = await api().saveDoc('practice', d);
		const i = all.findIndex((x) => x.name === saved.name);
		if (i >= 0) all[i] = saved;
		else all.push(saved);
		return saved;
	}

	$effect(() => {
		if (id) void load();
	});


</script>

<div class="section-row">
	<span class="section-title">Practices</span>
	<span class="hint">Standing rules agents always follow on this project, plus the global ones it inherits.</span>
	<span class="spacer"></span>
	<Button size="sm" data-testid="practices-new" onclick={openNew}>New practice</Button>
</div>

{#snippet rows(docs: Doc[], scope: 'project' | 'global', tableId: string)}
	<Table
		id={tableId}
		{columns}
		rows={docs}
		rowKey={(d: Doc) => d.id}
	>
		{#snippet cell(doc: Doc, column: TableColumn<Doc>)}
			{#if column.key === 'name'}
				<button type="button" class="link" onclick={() => openEdit(doc)}>{doc.name}</button>
			{:else if column.key === 'tags'}
				<span class="tags">
					{#each doc.tags as tag (tag)}<Badge mono>{tag}</Badge>{/each}
				</span>
			{:else if column.key === 'scope'}
				<Badge>{scope}</Badge>
			{:else}
				{relativeAge(doc.updated_at)}
			{/if}
		{/snippet}
		{#snippet empty()}
			{#if scope === 'project'}
				<div class="empty">
					<span class="title">No project practices yet</span>
					<span class="hint">A practice is a standing rule agents should follow, written in Markdown.</span>
				</div>
			{:else}
				<div class="empty">
					<span class="hint">No global practices.</span>
				</div>
			{/if}
		{/snippet}
	</Table>
{/snippet}

{#if error}
	<ErrorState message={error} logPath={logPath ?? undefined}>
		<Button variant="primary" onclick={() => load()}>Retry</Button>
	</ErrorState>
{:else}
	<div class="stack">
		{@render rows(split.project, 'project', 'project-practices')}

		<div class="card">
			<div class="head">
				<span class="group-heading">Inherited from global</span>
				<span class="spacer"></span>
				<span class="mono count">{split.inherited.length} practices</span>
			</div>
			{@render rows(split.inherited, 'global', 'project-practices-inherited')}
		</div>
	</div>
{/if}

<PracticeDialog {open} {editing} projectId={id} onclose={() => (open = false)} onsave={save} />

<style>
	.stack {
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.card {
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}

	.head {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.spacer {
		flex: 1;
	}

	.count {
		color: var(--text-tertiary);
	}

	.link {
		border: 0;
		background: none;
		padding: 0;
		color: var(--accent);
		font-family: var(--font-mono);
		cursor: pointer;
	}

	.link:hover {
		text-decoration: underline;
	}

	.tags {
		display: inline-flex;
		flex-wrap: wrap;
		gap: 4px;
	}

	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 6px;
		padding: 28px 0;
	}

	.title {
		font-weight: 600;
	}

	.hint {
		color: var(--text-secondary);
	}
	.section-row {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	.section-title {
		font-weight: var(--weight-semibold);
	}

	.spacer {
		flex: 1;
	}
</style>
