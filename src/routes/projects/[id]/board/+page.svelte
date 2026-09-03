<script lang="ts">
	// One project's board inside the shell: a column per stage, cards for the tasks that
	// pass the filter bar, and a drawer for the task in hand. The project comes from the
	// route, so there is no project picker here; `/projects/global/board` is the same
	// screen with no project filter at all.
	//
	// This is the Phase 4 board moved under the project route so the palette's task hits
	// have somewhere to land. Phase 8 replaces it with the frame's version.
	import { onMount, tick, untrack } from 'svelte';
	import { page } from '$app/state';
	import NewTaskDialog from '$lib/components/NewTaskDialog.svelte';
	import TaskCard from '$lib/components/TaskCard.svelte';
	import TaskDrawer from '$lib/components/TaskDrawer.svelte';
	import { plural } from '$lib/format';
	import { setStatusItems, shell, TabStrip, type Tab } from '$lib/shell';
	import {
		board,
		cancelRefresh,
		closeTask,
		columns,
		move,
		openTask,
		refresh,
		reload,
		scheduleRefresh
	} from '$lib/stores/board.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import Button from '$lib/ui/Button.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Input from '$lib/ui/Input.svelte';

	/** `global` is the every-project board, which the daemon means by no `project_id`. */
	const routeId = $derived(page.params.id ?? 'global');
	const projectId = $derived(routeId === 'global' ? null : routeId);
	const project = $derived(projects.items.find((p) => p.id === projectId) ?? null);
	const name = $derived(projectId ? (project?.name ?? 'Project') : 'Global');
	const root = $derived(project?.root_path ?? '');

	// Global has no profile page to tab back to, so its first tab is the project list.
	const tabs: Tab[] = $derived(
		projectId
			? [
					{ id: 'profile', label: 'Profile', icon: 'folder', href: `/projects/${projectId}` },
					{
						id: 'board',
						label: 'Board',
						icon: 'columns-3',
						href: `/projects/${projectId}/board`
					}
				]
			: [
					{ id: 'profile', label: 'Projects', icon: 'folder', href: '/projects' },
					{ id: 'board', label: 'Board', icon: 'columns-3', href: '/projects/global/board' }
				]
	);

	const stageColumns = $derived(columns());
	let creating = $state(false);

	/**
	 * The route and `?q=` own the filters, so a second palette action while this page is
	 * already open re-points it rather than being ignored. `untrack` keeps the writes and
	 * the fetch out of the dependency set: `refresh` reads every filter, and tracking
	 * those would turn each keystroke in the search box into a second request.
	 */
	$effect(() => {
		const id = projectId;
		const q = page.url.searchParams.get('q') ?? '';
		untrack(() => {
			board.filters.projectId = id;
			board.filters.query = q;
			closeTask();
			void refresh();
		});
	});

	// `?task=<key>` opens the drawer on that task, whether or not the list holds it.
	$effect(() => {
		const key = page.url.searchParams.get('task');
		untrack(() => {
			if (key && board.selected !== key) openTask(key);
		});
	});

	// `?new=1` opens the create dialog. Closing it does not re-run this.
	$effect(() => {
		const wanted = page.url.searchParams.get('new') === '1';
		untrack(() => {
			if (wanted) creating = true;
		});
	});

	// The side panel otherwise reads "Projects"; naming the open board is more useful.
	$effect(() => {
		shell.sidePanelTitle = projectId ? (project?.name ?? 'Projects') : 'Projects';
	});

	$effect(() => {
		setStatusItems({ right: [{ text: plural(board.tasks.length, 'task') }] });
	});

	/** Closes the drawer and hands focus back to the card it came from. */
	async function dismiss() {
		const key = board.selected;
		closeTask();
		if (!key) return;
		// The focused close button unmounts with the drawer, which would drop focus on
		// the body and restart the tab order at the top of the page.
		await tick();
		document.querySelector<HTMLElement>(`[data-testid="task-open-${key}"]`)?.focus();
	}

	onMount(() => {
		void loadProjects();
		// A pending debounce would fire a request for a screen that is gone.
		return cancelRefresh;
	});
</script>

<div class="head">
	<div class="ident">
		<span class="name">{name}</span>
		{#if root}<code class="root">{root}</code>{/if}
	</div>
	<TabStrip items={tabs} active="board" />
</div>

<div class="filters">
	<div class="pick">
		<Input
			bind:value={board.filters.assignee}
			data-testid="board-assignee"
			aria-label="Assignee"
			placeholder="Assignee"
			oninput={() => scheduleRefresh()}
		/>
	</div>

	<div class="search">
		<Input
			bind:value={board.filters.query}
			data-testid="board-search"
			aria-label="Search tasks"
			placeholder="Search tasks…"
			oninput={() => scheduleRefresh()}
		/>
	</div>

	<label class="toggle">
		<input
			type="checkbox"
			bind:checked={board.filters.showDone}
			data-testid="board-show-done"
			onchange={() => scheduleRefresh(0)}
		/>
		<span>Show done</span>
	</label>

	<Button variant="primary" data-testid="board-new" onclick={() => (creating = true)}>
		New task
	</Button>
</div>

{#if board.error}
	<ErrorState message={board.error} logPath={board.errorLogPath ?? undefined}>
		<Button variant="primary" onclick={() => refresh()}>Retry</Button>
	</ErrorState>
{:else if board.stages.length === 0}
	<EmptyState
		title={board.loading ? 'Loading…' : 'No stages yet'}
		hint="Set the board's stages in Settings, or on a project to give it its own."
	/>
{:else}
	<div class="split" class:drawer={!!board.selected}>
		<div class="columns">
			{#each stageColumns as column (column.stage.name)}
				<section class="column" data-testid="board-column-{column.stage.name}">
					<header>
						<h2>{column.stage.name} ({column.tasks.length - column.strayCount})</h2>
						{#if column.strays}
							<span class="note" title="These tasks are in a stage the board no longer has">
								plus {column.strayCount} from a removed stage
							</span>
						{/if}
					</header>

					{#each column.tasks as task (task.id)}
						<TaskCard
							{task}
							stages={board.stages}
							onopen={openTask}
							onmove={(key, stage) => void move(key, stage)}
						/>
					{:else}
						<p class="empty">Nothing here.</p>
					{/each}
				</section>
			{/each}
		</div>

		{#if board.selected}
			<TaskDrawer
				detail={board.detail}
				stages={board.stages}
				loading={board.detailLoading}
				error={board.detailError}
				onclose={dismiss}
				onchanged={reload}
				onmove={(key, stage) => void move(key, stage)}
				ondeleted={() => {
					closeTask();
					return refresh();
				}}
			/>
		{/if}
	</div>
{/if}

<NewTaskDialog
	open={creating}
	projectId={board.filters.projectId}
	projectName={projectId ? name : null}
	onclose={() => (creating = false)}
	oncreated={refresh}
/>

<style>
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-4);
		height: 28px;
		flex: 0 0 28px;
	}

	.ident {
		display: flex;
		align-items: baseline;
		gap: var(--space-2);
		min-width: 0;
	}

	.name {
		font-family: var(--font-mono);
		font-size: 15px;
		font-weight: 600;
	}

	.root {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text-secondary);
		font-size: 12px;
	}

	/* Wraps rather than squeezing: at a narrow width the controls compressed into
	   unusable slivers, so they drop onto a second row and keep a floor. */
	.filters {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: var(--space-3);
		flex: 0 0 auto;
	}

	.pick {
		flex: 0 1 180px;
		min-width: 140px;
	}

	.search {
		flex: 1 1 200px;
		min-width: 160px;
	}

	.toggle {
		display: flex;
		align-items: center;
		gap: var(--space-1);
		font-size: 13px;
		color: var(--text-secondary);
		white-space: nowrap;
	}

	.split {
		display: grid;
		gap: var(--space-4);
		align-items: start;
		min-height: 0;
		overflow: auto;
	}

	.split.drawer {
		grid-template-columns: 1fr 360px;
	}

	.columns {
		display: grid;
		grid-auto-flow: column;
		grid-auto-columns: minmax(240px, 1fr);
		gap: var(--space-3);
		overflow-x: auto;
		align-items: start;
	}

	.column {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-3);
		border: 1px solid var(--border-default);
		border-radius: var(--radius-sm);
		background: var(--bg-surface);
	}

	.column header {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.column h2 {
		margin: 0;
		font-size: 13px;
		text-transform: uppercase;
		letter-spacing: 0.04em;
		color: var(--text-secondary);
	}

	.note {
		font-size: 12px;
		color: var(--danger-text);
	}

	.empty {
		margin: 0;
		padding: var(--space-3) 0;
		color: var(--text-secondary);
		font-size: 13px;
		text-align: center;
	}
</style>
