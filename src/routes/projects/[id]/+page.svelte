<script lang="ts">
	// One project: its cached profile, and the context an agent would receive for
	// this root (memories, practices, workflows). Refresh rebuilds the profile.
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import StageEditor from '$lib/components/StageEditor.svelte';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { relativeAge } from '$lib/format';
	import { shell, TabStrip, type Tab } from '$lib/shell';
	import {
		deleteProject,
		loadProject,
		projectDetail,
		refreshProject
	} from '$lib/stores/projects.svelte';
	import type { Doc, RecallHit, Stage } from '$lib/types';
	import Badge from '$lib/ui/Badge.svelte';
	import Button from '$lib/ui/Button.svelte';
	import Card from '$lib/ui/Card.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Table from '$lib/ui/Table.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	const id = $derived(page.params.id ?? '');
	const project = $derived(projectDetail.project);
	const profile = $derived(project?.profile ?? null);
	const context = $derived(projectDetail.context);

	/** The board is the other half of a project, so the header carries a tab to it. */
	const tabs: Tab[] = $derived([
		{ id: 'profile', label: 'Profile', icon: 'folder', href: `/projects/${id}` },
		{ id: 'board', label: 'Board', icon: 'columns-3', href: `/projects/${id}/board` }
	]);

	const memoryColumns = [
		{ key: 'kind', label: 'Kind', width: '110px' },
		{ key: 'text', label: 'Text' },
		{ key: 'age', label: 'Age', width: '80px', align: 'right' as const }
	];

	const docColumns = [
		{ key: 'name', label: 'Name', width: '220px' },
		{ key: 'tags', label: 'Tags' },
		{ key: 'age', label: 'Updated', width: '90px', align: 'right' as const }
	];

	// Re-fetches when the route parameter changes.
	$effect(() => {
		if (id) {
			void loadProject(id);
			void loadStages(id);
		}
	});

	// The side panel otherwise reads "Projects" here too; naming the open project is
	// more useful than repeating the rail label.
	$effect(() => {
		shell.sidePanelTitle = project?.name ?? 'Projects';
	});

	let confirming = $state(false);

	/** The stages this project works to, and whether they are its own. */
	let stages = $state<Stage[]>([]);
	let overridden = $state(false);
	let stagesError = $state<string | null>(null);

	async function loadStages(projectId: string) {
		try {
			const list = await api().boardStages(projectId);
			stages = list.stages;
			overridden = list.overridden;
			stagesError = null;
		} catch (e) {
			stages = [];
			stagesError = errorMessage(e);
		}
	}

	async function saveStages(next: Stage[], renames: Record<string, string>) {
		try {
			const list = await api().setProjectStages(id, next, renames);
			stages = list.stages;
			overridden = list.overridden;
			stagesError = null;
			push('success', 'Board stages saved');
		} catch (e) {
			stagesError = errorMessage(e);
			push('error', stagesError);
		}
	}

	/** Drops the override so the project follows the global list again. */
	async function useGlobalStages() {
		try {
			const list = await api().setProjectStages(id, null);
			stages = list.stages;
			overridden = list.overridden;
			stagesError = null;
			push('success', 'Using the global stages');
		} catch (e) {
			stagesError = errorMessage(e);
			push('error', stagesError);
		}
	}

	async function refresh() {
		try {
			await refreshProject(id);
			push('success', 'Profile rebuilt');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	async function confirmRemove() {
		const name = project?.name ?? 'Project';
		try {
			await deleteProject(id);
			confirming = false;
			// The page's own project is gone, so leave before the detail state is read again.
			await goto('/projects');
			push('success', `Removed ${name}`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

<div class="head">
	<div>
		<a class="back" href="/projects">← Projects</a>
		<h1>{project?.name ?? 'Project'}</h1>
		{#if project}
			<p class="sub">
				<code>{project.root_path}</code>
				<span data-testid="project-board-key">
					· Board key {project.board_key ?? 'not assigned yet'}
				</span>
			</p>
		{/if}
	</div>
	<div class="actions">
		<TabStrip items={tabs} active="profile" />
		<Button
			variant="primary"
			data-testid="project-refresh"
			disabled={!project || projectDetail.refreshing}
			onclick={refresh}
		>
			{projectDetail.refreshing ? 'Refreshing…' : 'Refresh'}
		</Button>
		<Button
			variant="danger"
			data-testid="project-remove"
			disabled={!project || projectDetail.removing}
			onclick={() => (confirming = true)}
		>
			Remove
		</Button>
	</div>
</div>

<Dialog open={confirming} title="Remove this project?" onclose={() => (confirming = false)}>
	<p class="prose">
		Atlas forgets <strong>{project?.name ?? 'this project'}</strong> and stops offering it as
		a scope. Its memories are kept, its tasks are deleted, and connecting the same root again
		re-adds it. Nothing on disk is touched.
	</p>
	{#snippet footer()}
		<Button onclick={() => (confirming = false)}>Cancel</Button>
		<Button
			variant="danger"
			data-testid="project-remove-confirm"
			disabled={projectDetail.removing}
			onclick={confirmRemove}
		>
			{projectDetail.removing ? 'Removing…' : 'Remove'}
		</Button>
	{/snippet}
</Dialog>

{#if projectDetail.error}
	<ErrorState message={projectDetail.error} logPath={projectDetail.errorLogPath ?? undefined}>
		<Button variant="primary" onclick={() => loadProject(id)}>Retry</Button>
	</ErrorState>
{:else if projectDetail.loading && !project}
	<p class="muted">Loading…</p>
{:else if !project}
	<EmptyState title="Project not found" hint="It may have been removed since the list loaded." />
{:else}
	<div class="stack">
		<Card title="Profile" data-testid="project-profile">
			{#if !profile}
				<EmptyState
					title="No profile yet"
					hint="Refresh scans the repository for languages, frameworks and recent commits."
				/>
			{:else}
				<dl>
					<dt>Remote</dt>
					<dd>{project.git_remote ?? '-'}</dd>
					<dt>Last seen</dt>
					<dd>{new Date(project.last_seen_at).toLocaleString()}</dd>
					<dt>Built</dt>
					<dd>{new Date(profile.built_at).toLocaleString()}</dd>
					<dt>Languages</dt>
					<dd>
						{#each profile.languages as lang (lang)}<Badge tone="accent">{lang}</Badge>{:else}-{/each}
					</dd>
					<dt>Frameworks</dt>
					<dd>
						{#each profile.frameworks as fw (fw)}<Badge tone="accent">{fw}</Badge>{:else}-{/each}
					</dd>
				</dl>

				{#if profile.summary}
					<h3>Summary</h3>
					<p class="prose">{profile.summary}</p>
				{/if}

				{#if profile.tree.length > 0}
					<h3>Tree</h3>
					<pre class="block">{profile.tree.join('\n')}</pre>
				{/if}

				{#if profile.readme_head}
					<h3>README</h3>
					<pre class="block">{profile.readme_head}</pre>
				{/if}

				{#if profile.recent_commits.length > 0}
					<h3>Recent commits</h3>
					<ul class="commits">
						{#each profile.recent_commits as commit, i (i)}
							<li><code>{commit}</code></li>
						{/each}
					</ul>
				{/if}
			{/if}
		</Card>

		<Card title="Memories" data-testid="project-memories">
			<Table
				columns={memoryColumns}
				rows={context?.memories ?? []}
				rowKey={(hit: RecallHit) => hit.memory.id}
			>
				{#snippet cell(hit: RecallHit, key: string)}
					{#if key === 'kind'}
						<Badge>{hit.memory.kind}</Badge>
					{:else if key === 'text'}
						<span class="text">{hit.memory.text}</span>
					{:else}
						<span class="muted">{relativeAge(hit.memory.created_at)}</span>
					{/if}
				{/snippet}
				{#snippet empty()}
					<EmptyState
						title="No memories for this project"
						hint="Project-scoped memories appear here as agents record them."
					/>
				{/snippet}
			</Table>
		</Card>

		<Card title="Board stages" data-testid="project-stages">
			{#snippet actions()}
				<Button size="sm" data-testid="project-stages-global" disabled={!overridden} onclick={useGlobalStages}>
					Use global stages
				</Button>
			{/snippet}
			<p class="hint">
				{overridden
					? 'This project has its own stages. Saving keeps the override.'
					: 'This project follows the global stages. Saving gives it its own.'}
			</p>
			{#if stagesError}
				<p class="bad" role="alert" data-testid="project-stages-error">{stagesError}</p>
			{/if}
			<StageEditor stages={stages} onsave={saveStages} />
		</Card>

		<Card title="Practices" data-testid="project-practices">
			{@render docTable(context?.practices ?? [], 'practices')}
		</Card>

		<Card title="Workflows" data-testid="project-workflows">
			{@render docTable(context?.workflows ?? [], 'workflows')}
		</Card>
	</div>
{/if}

{#snippet docTable(docs: Doc[], label: string)}
	<Table columns={docColumns} rows={docs} rowKey={(d: Doc) => d.id}>
		{#snippet cell(doc: Doc, key: string)}
			{#if key === 'name'}
				{doc.name}
			{:else if key === 'tags'}
				{#each doc.tags as tag (tag)}<Badge tone="accent">{tag}</Badge>{:else}
					<span class="muted">-</span>
				{/each}
			{:else}
				<span class="muted">{relativeAge(doc.updated_at)}</span>
			{/if}
		{/snippet}
		{#snippet empty()}
			<EmptyState title="No {label}" hint="Global {label} still apply to this project." />
		{/snippet}
	</Table>
{/snippet}

<style>
	.head {
		display: flex;
		align-items: flex-start;
		justify-content: space-between;
		gap: var(--space-4);
		margin-bottom: var(--space-4);
	}

	.head h1 {
		margin: 0;
		height: 28px;
		line-height: 28px;
		font-size: 15px;
		font-weight: 600;
	}

	.actions {
		display: flex;
		align-items: center;
		height: 28px;
		gap: var(--space-2);
	}

	.back {
		display: block;
		margin-bottom: var(--space-1);
		color: var(--text-secondary);
		font-size: 13px;
	}

	.sub {
		margin: var(--space-1) 0 0;
		color: var(--text-secondary);
	}

	.stack {
		display: flex;
		flex-direction: column;
		gap: var(--space-4);
	}

	dl {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: var(--space-1) var(--space-3);
		margin: 0;
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

	h3 {
		margin: var(--space-4) 0 var(--space-2);
		font-size: 11px;
		font-weight: 600;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--text-tertiary);
	}

	.prose {
		margin: 0;
		max-width: 80ch;
	}

	.block {
		margin: 0;
		max-height: 320px;
		overflow: auto;
		padding: var(--space-3);
		border: 1px solid var(--border-default);
		border-radius: var(--radius-sm);
		background: var(--bg-base);
		font-family: var(--font-mono);
		font-size: 12px;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.commits {
		margin: 0;
		padding-left: var(--space-4);
		font-size: 13px;
	}

	.text {
		display: block;
		max-width: 70ch;
		overflow-wrap: anywhere;
	}

	.muted {
		color: var(--text-secondary);
	}

	.hint {
		margin: 0 0 var(--space-3);
		color: var(--text-secondary);
		font-size: 13px;
	}

	.bad {
		margin: 0 0 var(--space-2);
		color: var(--danger-text);
	}
</style>
