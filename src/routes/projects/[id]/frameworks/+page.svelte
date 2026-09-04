<script lang="ts">
	// The Frameworks tab: what Atlas detected among the planning frameworks it understands
	// (Superpowers, OpenSpec, SpecKit, GSD) in this project's root, one card per framework
	// plus a shared documents table with a preview pane, and the import actions that turn
	// a framework's tasks and decisions into board tasks and pending memories.
	import { Badge, Button, Select, Table, type TableColumn } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorLogPath, errorMessage } from '$lib/errors';
	import { dateTime, relativeAge } from '$lib/format';
	import { board, refresh as refreshBoard } from '$lib/stores/board.svelte';
	import { project, setHeaderActions } from '$lib/stores/project.svelte';
	import {
		DOC_TYPE_LABEL,
		DOC_TYPE_TONE,
		docRowKey,
		documentRows,
		FRAMEWORK_LABEL,
		reportText
	} from '$lib/components/project/frameworks';
	import type { FrameworkDoc, FrameworkKind, FrameworkListing, ImportWhat } from '$lib/types';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	const id = $derived(project.current?.id ?? '');

	let listings = $state<FrameworkListing[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let logPath = $state<string | null>(null);

	// Held as a plain string because `Select` binds one; only ever a `FrameworkKind`,
	// cast at the point of use, the same way the task detail panel handles its Selects.
	let importKind = $state('');
	let importing = $state(false);

	let selected = $state<FrameworkDoc | null>(null);
	let docContent = $state<string | null>(null);
	let docLoading = $state(false);
	let docError = $state<string | null>(null);

	const rows = $derived(documentRows(listings));
	const kindOptions = $derived(
		listings.map((l) => ({ value: l.inventory.kind, label: FRAMEWORK_LABEL[l.inventory.kind] }))
	);

	const columns: TableColumn<FrameworkDoc>[] = [
		{ key: 'doc_type', label: 'Type', width: '110px' },
		{ key: 'title', label: 'Title' },
		{ key: 'path', label: 'Path', mono: true, width: '300px' },
		{ key: 'updated_at', label: 'Updated', width: '90px', align: 'right' }
	];

	async function load(): Promise<void> {
		if (!id) return;
		loading = true;
		try {
			listings = await api().listFrameworks(id);
			error = null;
			logPath = null;
			// Keeps a stale selection (a kind removed since the last load) from silently
			// importing the wrong framework.
			if (!listings.some((l) => l.inventory.kind === importKind)) {
				importKind = listings[0]?.inventory.kind ?? '';
			}
		} catch (e) {
			listings = [];
			error = errorMessage(e);
			logPath = errorLogPath(e);
		} finally {
			loading = false;
		}
	}

	async function selectDoc(doc: FrameworkDoc): Promise<void> {
		selected = doc;
		docContent = null;
		docError = null;
		docLoading = true;
		try {
			docContent = await api().getFrameworkDoc(id, doc.kind, doc.path);
		} catch (e) {
			docError = errorMessage(e);
		} finally {
			docLoading = false;
		}
	}

	async function doImport(what: ImportWhat): Promise<void> {
		const kind = (importKind || listings[0]?.inventory.kind) as FrameworkKind | undefined;
		if (!id || !kind) return;
		importing = true;
		try {
			const report = await api().importFramework(id, kind, what);
			push('success', reportText(report));
			// The Board tab reads its own module state rather than refetching on focus, so
			// an import from here has to refresh it directly for the board's counts to
			// reflect the tasks this just created.
			board.filters.projectId = id;
			await refreshBoard();
			await load();
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			importing = false;
		}
	}

	// A different project's own effect run (not `doImport`'s manual reload), so the
	// preview is cleared only on an actual project change, not on every reload.
	$effect(() => {
		if (!id) return;
		selected = null;
		docContent = null;
		docError = null;
		void load();
	});

	$effect(() => {
		setHeaderActions(listings.length > 0 ? headerActions : null);
		return () => setHeaderActions(null);
	});
</script>

{#snippet headerActions()}
	{#if kindOptions.length > 1}
		<Select
			size="sm"
			bind:value={importKind}
			options={kindOptions}
			aria-label="Framework to import"
			data-testid="frameworks-kind-select"
		/>
	{/if}
	<Button
		data-testid="frameworks-import-tasks"
		disabled={importing}
		onclick={() => doImport('tasks')}
	>
		{importing ? 'Importing…' : 'Import tasks'}
	</Button>
	<Button
		data-testid="frameworks-import-decisions"
		disabled={importing}
		onclick={() => doImport('decisions')}
	>
		Import decisions
	</Button>
{/snippet}

{#if error}
	<ErrorState message={error} logPath={logPath ?? undefined}>
		<Button variant="primary" onclick={() => load()}>Retry</Button>
	</ErrorState>
{:else if !loading && listings.length === 0}
	<div class="card pad empty-frameworks" data-testid="frameworks-empty">
		<span class="group-heading">No framework detected</span>
		<p class="hint">Atlas looks for these folders in the project root:</p>
		<ul class="folders">
			<li>
				<span class="mono">docs/superpowers/specs</span>, <span class="mono">docs/superpowers/plans</span>,
				<span class="mono">.superpowers/sdd</span> &middot; Superpowers
			</li>
			<li><span class="mono">openspec/specs</span>, <span class="mono">openspec/changes</span> &middot; OpenSpec</li>
			<li><span class="mono">.specify</span>, <span class="mono">specs</span> &middot; SpecKit</li>
			<li><span class="mono">.planning</span> &middot; GSD</li>
		</ul>
	</div>
{:else}
	<div class="stack">
		<div class="cards">
			{#each listings as listing (listing.inventory.kind)}
				<div class="card pad" data-testid="framework-card-{listing.inventory.kind}">
					<span class="group-heading head-row">{FRAMEWORK_LABEL[listing.inventory.kind]}</span>
					<div class="reading">
						<span class="label">Roots</span>
						<span class="mono value">{listing.inventory.roots.join(', ')}</span>
					</div>
					<div class="reading">
						<span class="label">Documents</span>
						<span class="value">{listing.inventory.docs}</span>
					</div>
					<div class="reading">
						<span class="label">Tasks</span>
						<span class="value">{listing.inventory.tasks}</span>
					</div>
					<div class="reading">
						<span class="label">Detected</span>
						<span class="mono value">{dateTime(listing.inventory.detected_at)}</span>
					</div>
				</div>
			{/each}
		</div>

		<div class="panes">
			<section class="card pane" data-testid="frameworks-documents">
				<Table
					id="project-frameworks-docs"
					{columns}
					{rows}
					rowKey={docRowKey}
					onRowClick={selectDoc}
					selectedKey={selected ? docRowKey(selected) : null}
				>
					{#snippet cell(row: FrameworkDoc, column: TableColumn<FrameworkDoc>)}
						{#if column.key === 'doc_type'}
							<Badge tone={DOC_TYPE_TONE[row.doc_type]}>{DOC_TYPE_LABEL[row.doc_type]}</Badge>
						{:else if column.key === 'updated_at'}
							{relativeAge(row.updated_at)}
						{:else if column.key === 'title'}
							{row.title}
						{:else}
							{row.path}
						{/if}
					{/snippet}
					{#snippet empty()}
						<div class="empty"><span>{loading ? 'Loading…' : 'No documents yet.'}</span></div>
					{/snippet}
				</Table>
			</section>

			<section class="card pane" data-testid="frameworks-preview">
				<header>
					<span class="group-heading">Preview</span>
					<span class="spacer"></span>
					{#if selected}<span class="mono meta">{selected.path}</span>{/if}
				</header>
				<div class="scroll readme">
					{#if !selected}
						<p class="empty-text">Select a document to preview it.</p>
					{:else if docLoading}
						<p class="empty-text">Loading…</p>
					{:else if docError}
						<p class="bad">{docError}</p>
					{:else}
						<pre class="mono">{docContent}</pre>
					{/if}
				</div>
			</section>
		</div>
	</div>
{/if}

<style>
	.stack {
		display: flex;
		flex-direction: column;
		gap: 12px;
		min-height: 0;
		flex: 1;
	}

	.cards {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
		gap: 12px;
		flex: 0 0 auto;
	}

	.card {
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
	}

	.pad {
		display: flex;
		flex-direction: column;
		padding: 8px 12px;
	}

	.head-row {
		display: flex;
		align-items: center;
		height: 22px;
		flex: 0 0 22px;
	}

	.reading {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 22px;
		flex: 0 0 22px;
		color: var(--text-secondary);
	}

	.label {
		width: 90px;
		flex: 0 0 90px;
	}

	.value {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text-primary);
	}

	.panes {
		flex: 1;
		display: grid;
		grid-template-columns: 1.4fr 1fr;
		gap: 12px;
		min-height: 0;
	}

	.pane {
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow: hidden;
	}

	.pane header {
		display: flex;
		align-items: center;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.spacer {
		flex: 1;
	}

	.meta {
		color: var(--text-tertiary);
	}

	.scroll {
		flex: 1;
		min-height: 0;
		overflow: auto;
	}

	.readme {
		padding: 8px 12px;
	}

	.readme pre {
		margin: 0;
		font-size: 12px;
		line-height: 18px;
		color: var(--text-secondary);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.empty-text,
	.bad {
		margin: 0;
		padding: 8px 12px;
		color: var(--text-tertiary);
	}

	.bad {
		color: var(--danger-text);
	}

	.empty {
		display: flex;
		align-items: center;
		justify-content: center;
		padding: 20px 0;
		color: var(--text-secondary);
	}

	.empty-frameworks {
		gap: 8px;
	}

	.hint {
		margin: 0;
		color: var(--text-secondary);
	}

	.folders {
		margin: 0;
		padding-left: 18px;
		display: flex;
		flex-direction: column;
		gap: 4px;
		color: var(--text-secondary);
	}
</style>
