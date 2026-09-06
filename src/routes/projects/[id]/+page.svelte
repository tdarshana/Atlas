<script lang="ts">
	// The Profile tab: what Atlas knows about this root. Two reading cards across the top,
	// then the three dense panes the frame calls TREE, README and RECENT COMMITS.
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { Badge, Button, Icon } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { dateTime, NOTHING, plural } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import {
		activeAgents,
		project,
		refresh,
		remove,
		setHeaderActions,
		taskSummary,
		treeRows
	} from '$lib/stores/project.svelte';
	import { projects } from '$lib/stores/projects.svelte';
	import type { Task } from '$lib/types';
	import RemoveProjectDialog from '$lib/components/project/RemoveProjectDialog.svelte';
	import MarkdownView from '$lib/ui/MarkdownView.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	let tasks = $state<Task[]>([]);
	let confirming = $state(false);

	const id = $derived(page.params.id ?? '');
	const current = $derived(project.current);
	const profile = $derived(current?.profile ?? null);
	const rows = $derived(treeRows(profile?.tree ?? []));
	const commits = $derived(profile?.recent_commits ?? []);
	const agents = $derived(activeAgents(tasks));

	/** A profile older than an hour is rebuilt quietly once per visit, so the remote,
	 * languages and tree keep up without a click on Refresh. */
	let refreshedFor: string | null = null;
	$effect(() => {
		const built = current?.profile?.built_at;
		if (!id || !built || refreshedFor === id || project.refreshing) return;
		if (Date.now() - Date.parse(built) < 60 * 60 * 1000) return;
		refreshedFor = id;
		void refresh().catch(() => {});
	});

	// The stack card counts this project's tasks, which the board route does not preload.
	$effect(() => {
		if (!id) return;
		let live = true;
		void api()
			.listTasks({ project_id: id, include_done: true })
			.then((list) => {
				if (live) tasks = list;
			})
			.catch(() => {
				if (live) tasks = [];
			});
		return () => {
			live = false;
		};
	});

	$effect(() => {
		const name = current?.name ?? 'Atlas';
		setStatusItems({ right: [{ text: `${name} · ${plural(projects.items.length, 'project')}` }] });
	});

	// The header is the layout's; the tab lends it the buttons for as long as it is open.
	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});

	async function rebuild() {
		try {
			await refresh();
			push('success', 'Profile rebuilt');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	async function confirmRemove() {
		const name = current?.name ?? 'Project';
		try {
			await remove();
			confirming = false;
			// This page's project is gone, so leave before its state is read again.
			await goto('/projects');
			push('success', `Removed ${name}`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

{#snippet headerActions()}
	<Button data-testid="project-refresh" disabled={!current || project.refreshing} onclick={rebuild}>
		{project.refreshing ? 'Refreshing…' : 'Refresh'}
	</Button>
	<Button
		variant="danger"
		data-testid="project-remove"
		disabled={!current || project.removing}
		onclick={() => (confirming = true)}
	>
		Remove…
	</Button>
{/snippet}

{#snippet reading(label: string, value: string)}
	<div class="reading">
		<span class="label">{label}</span>
		<!-- An empty reading takes the token the design system keeps for one, so "Remote ·"
		     does not read with the weight of a real remote. -->
		<span class="mono value" class:nothing={value === NOTHING}>{value}</span>
	</div>
{/snippet}

<RemoveProjectDialog
	open={confirming}
	name={current?.name ?? 'this project'}
	testid="project-remove-confirm"
	onclose={() => (confirming = false)}
	onconfirm={confirmRemove}
/>

<div class="cards">
	<div class="card pad" data-testid="project-profile">
		<span class="group-heading head-row">Profile</span>
		{@render reading('Remote', current?.git_remote ?? NOTHING)}
		{@render reading('Last seen', dateTime(current?.last_seen_at))}
		{@render reading('Built', dateTime(profile?.built_at))}
		{@render reading('Key prefix', current?.board_key ?? NOTHING)}
	</div>

	<div class="card pad" data-testid="project-stack">
		<span class="group-heading head-row">Stack</span>
		<div class="reading">
			<span class="label">Languages</span>
			<div class="badges">
				{#each profile?.languages ?? [] as lang (lang)}
					<Badge tone="accent" mono>{lang}</Badge>
				{:else}
					<span class="mono value nothing">{NOTHING}</span>
				{/each}
			</div>
		</div>
		<div class="reading">
			<span class="label">Frameworks</span>
			<div class="badges">
				{#each profile?.frameworks ?? [] as fw (fw)}
					<Badge tone="accent" mono>{fw}</Badge>
				{:else}
					<span class="mono value nothing">{NOTHING}</span>
				{/each}
			</div>
		</div>
		<div class="reading">
			<span class="label">Agents</span>
			<div class="badges">
				{#each agents as agent (agent)}
					<span class="mono value">{agent}</span>
					<Badge tone="success">active</Badge>
				{:else}
					<span class="mono value nothing">{NOTHING}</span>
				{/each}
			</div>
		</div>
		{@render reading('Tasks', taskSummary(tasks))}
	</div>
</div>

<div class="panes">
	<section class="card pane" data-testid="project-tree">
		<header>
			<span class="group-heading">Tree</span>
			<span class="spacer"></span>
			<span class="mono meta">{plural(rows.length, 'entry', 'entries')}</span>
		</header>
		<div class="scroll rows">
			{#each rows as row (row.path)}
				<div class="tree-row mono" style="padding-left:{8 + row.depth * 16}px">
					<Icon
						name={row.kind === 'folder' ? 'folder' : 'file'}
						size={13}
						color="var(--text-tertiary)"
					/>
					<span class="clip">{row.name}</span>
				</div>
			{:else}
				<p class="empty">No tree yet. Refresh scans this root.</p>
			{/each}
		</div>
	</section>

	<section class="card pane" data-testid="project-readme">
		{#if profile?.readme_head}
			<MarkdownView source={profile.readme_head} path="README.md" />
		{:else}
			<header>
				<span class="group-heading">Readme</span>
				<span class="spacer"></span>
				<span class="mono meta">README.md</span>
			</header>
			<div class="scroll readme">
				<p class="empty">No readme in this root.</p>
			</div>
		{/if}
	</section>

	<section class="card pane" data-testid="project-commits">
		<header>
			<span class="group-heading">Recent commits</span>
			<span class="spacer"></span>
			<span class="mono meta">main</span>
		</header>
		<div class="scroll rows">
			{#each commits as commit, i (i)}
				<div class="commit">
					<Icon name="git-commit-horizontal" size={12} color="var(--text-tertiary)" />
					<span class="clip">{commit}</span>
				</div>
			{:else}
				<p class="empty">No commits read from this root.</p>
			{/each}
		</div>
	</section>
</div>

<style>
	.cards {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 12px;
		flex: 0 0 auto;
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

	.value.nothing {
		color: var(--text-null);
	}

	.badges {
		display: flex;
		align-items: center;
		gap: 4px;
		min-width: 0;
		overflow: hidden;
	}

	.panes {
		flex: 1;
		display: grid;
		grid-template-columns: 1fr 1.2fr 1.2fr;
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

	.rows {
		padding: 4px 0;
	}

	.tree-row {
		display: flex;
		align-items: center;
		gap: 6px;
		height: 22px;
		padding-right: 8px;
		color: var(--text-secondary);
	}

	.commit {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 22px;
		padding: 0 12px;
		color: var(--text-secondary);
	}

	.clip {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.readme {
		padding: 8px 12px;
	}

	.empty {
		margin: 0;
		padding: 8px 12px;
		color: var(--text-tertiary);
	}

</style>
