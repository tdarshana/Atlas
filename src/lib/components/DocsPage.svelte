<script lang="ts">
	// Practices and workflows differ only in their kind, so both routes render this
	// list plus editor dialog against the store built for their kind.

	import { onMount } from 'svelte';
	import { api, daemon } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { setStatusItems } from '$lib/shell';
	import { nameError, parseList } from '$lib/stores/agents.svelte';
	import type { DocsStore } from '$lib/stores/docs.svelte';
	import type { Doc, Project } from '$lib/types';
	import Badge from '$lib/ui/Badge.svelte';
	import Button from '$lib/ui/Button.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Input from '$lib/ui/Input.svelte';
	import Select from '$lib/ui/Select.svelte';
	import Table, { type TableColumn } from '$lib/ui/Table.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Props {
		store: DocsStore;
		/** Plural heading, e.g. "Practices". */
		title: string;
		/** Singular noun used in buttons and dialogs, e.g. "practice". */
		noun: string;
		hint: string;
	}

	let { store, title, noun, hint }: Props = $props();

	const GLOBAL = '';

	const COLUMNS: TableColumn[] = [
		{ key: 'name', label: 'Name', width: '25%' },
		{ key: 'tags', label: 'Tags', width: '25%' },
		{ key: 'project', label: 'Project', width: '25%' },
		{ key: 'actions', label: '', width: '90px', align: 'right' }
	];

	let projects = $state<Project[]>([]);

	let editing = $state<Doc | null>(null);
	let open = $state(false);
	let confirming = $state<Doc | null>(null);

	let name = $state('');
	let body = $state('');
	let tags = $state('');
	let projectId = $state(GLOBAL);
	let saving = $state(false);
	let formError = $state<string | null>(null);

	const invalid = $derived(nameError(name));

	const projectOptions = $derived([
		{ value: GLOBAL, label: 'Global (no project)' },
		...projects.map((p) => ({ value: p.id, label: p.name }))
	]);

	onMount(() => {
		void store.load();
		void api()
			.listProjects()
			.then((p) => (projects = p))
			// The list still works without projects; the select just has no entries.
			.catch(() => {});
	});

	$effect(() => {
		setStatusItems({ right: [{ text: `${store.state.list.length} ${title.toLowerCase()}` }] });
	});

	function projectName(id: string | null): string {
		if (id === null) return '-';
		return projects.find((p) => p.id === id)?.name ?? id;
	}

	function openNew(): void {
		editing = null;
		name = '';
		body = '';
		tags = '';
		projectId = GLOBAL;
		formError = null;
		open = true;
	}

	function openEdit(doc: Doc): void {
		editing = doc;
		name = doc.name;
		body = doc.body;
		tags = doc.tags.join(', ');
		projectId = doc.project_id ?? GLOBAL;
		formError = null;
		open = true;
	}

	async function save(): Promise<void> {
		if (invalid) {
			formError = invalid;
			return;
		}
		saving = true;
		formError = null;
		try {
			await store.save({
				name,
				body,
				tags: parseList(tags),
				project_id: projectId === GLOBAL ? null : projectId
			});
			push('success', `Saved ${name}`);
			open = false;
		} catch (e) {
			// A 400 carries the daemon's own message; show it as it came.
			formError = errorMessage(e);
			push('error', formError);
		} finally {
			saving = false;
		}
	}

	async function remove(): Promise<void> {
		const doc = confirming;
		confirming = null;
		if (!doc) return;
		try {
			await store.remove(doc.name);
			push('success', `Deleted ${doc.name}`);
			if (editing?.name === doc.name) open = false;
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

<header class="head">
	<h1>{title}</h1>
	<Button variant="primary" data-testid="doc-new" onclick={openNew}>New {noun}</Button>
</header>

{#if store.state.error}
	<ErrorState message={store.state.error} logPath={daemon.logPath || undefined}>
		<Button onclick={() => store.load()}>Retry</Button>
	</ErrorState>
{:else}
	<Table
		columns={COLUMNS}
		rows={store.state.list}
		rowKey={(d) => d.id}
		data-testid="docs-table"
		onrowclick={openEdit}
	>
		{#snippet cell(doc: Doc, key: string)}
			{#if key === 'name'}
				<strong>{doc.name}</strong>
			{:else if key === 'tags'}
				<span class="tags">
					{#each doc.tags as tag (tag)}
						<Badge>{tag}</Badge>
					{/each}
				</span>
			{:else if key === 'project'}
				<span class="muted">{projectName(doc.project_id)}</span>
			{:else}
				<Button
					size="sm"
					variant="danger"
					data-testid="doc-delete"
					onclick={(e) => {
						e.stopPropagation();
						confirming = doc;
					}}
				>
					Delete
				</Button>
			{/if}
		{/snippet}
		{#snippet empty()}
			<EmptyState title={store.state.loading ? 'Loading…' : `No ${title.toLowerCase()} yet`} {hint}>
				{#if !store.state.loading}
					<Button variant="primary" onclick={openNew}>New {noun}</Button>
				{/if}
			</EmptyState>
		{/snippet}
	</Table>
{/if}

<Dialog
	{open}
	title={editing ? `Edit ${editing.name}` : `New ${noun}`}
	onclose={() => (open = false)}
>
	<div class="form">
		<label class="field">
			<span>Name</span>
			<Input bind:value={name} disabled={!!editing} placeholder="commit-style" data-testid="doc-name" />
			{#if !editing && name !== '' && invalid}
				<span class="bad">{invalid}</span>
			{/if}
		</label>

		<label class="field">
			<span>Body (Markdown)</span>
			<Textarea bind:value={body} mono rows={12} data-testid="doc-body" />
		</label>

		<label class="field">
			<span>Tags</span>
			<Input bind:value={tags} placeholder="git, review" />
		</label>

		<label class="field">
			<span>Project</span>
			<Select bind:value={projectId} options={projectOptions} />
		</label>

		{#if formError}
			<p class="bad" role="alert" data-testid="doc-error">{formError}</p>
		{/if}
	</div>

	{#snippet footer()}
		<Button onclick={() => (open = false)}>Cancel</Button>
		<Button variant="primary" data-testid="doc-save" disabled={saving} onclick={save}>Save</Button>
	{/snippet}
</Dialog>

<Dialog
	open={confirming !== null}
	title={`Delete ${noun}`}
	onclose={() => (confirming = null)}
>
	<p>Delete <strong>{confirming?.name}</strong>?</p>
	{#snippet footer()}
		<Button onclick={() => (confirming = null)}>Cancel</Button>
		<Button variant="danger" data-testid="doc-delete-confirm" onclick={remove}>Delete</Button>
	{/snippet}
</Dialog>

<style>
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-3);
		height: 28px;
		flex: 0 0 28px;
		margin-bottom: var(--space-4);
	}

	h1 {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
	}

	.tags {
		display: inline-flex;
		flex-wrap: wrap;
		gap: var(--space-1);
	}

	.muted {
		color: var(--text-secondary);
	}

	.form {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
	}

	.field > span {
		font-size: 13px;
		color: var(--text-secondary);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}
</style>
