<script lang="ts">
	// Board: one column per stage of the chosen project's list, cards for the tasks
	// that pass the filter bar, and a drawer for the task in hand.
	import { onMount, tick } from 'svelte';
	import NewTaskDialog from '$lib/components/NewTaskDialog.svelte';
	import TaskCard from '$lib/components/TaskCard.svelte';
	import TaskDrawer from '$lib/components/TaskDrawer.svelte';
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
	import Select from '$lib/ui/Select.svelte';

	const projectOptions = $derived([
		{ value: '', label: 'All projects' },
		...projects.items.map((p) => ({ value: p.id, label: p.name }))
	]);

	const stageColumns = $derived(columns());
	let creating = $state(false);

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

	function onProject(value: string) {
		board.filters.projectId = value || null;
		closeTask();
		void refresh();
	}

	onMount(() => {
		void loadProjects();
		void refresh();
		// A pending debounce would fire a request for a screen that is gone.
		return cancelRefresh;
	});
</script>

<h1>Board</h1>

<div class="filters">
	<div class="pick">
		<Select
			value={board.filters.projectId ?? ''}
			options={projectOptions}
			aria-label="Project"
			data-testid="board-project"
			onchange={(e: Event & { currentTarget: HTMLSelectElement }) => onProject(e.currentTarget.value)}
		/>
	</div>

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
	onclose={() => (creating = false)}
	oncreated={refresh}
/>

<style>
	.filters {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		margin-bottom: var(--space-4);
	}

	.pick {
		width: 180px;
	}

	.search {
		flex: 1;
	}

	.toggle {
		display: flex;
		align-items: center;
		gap: var(--space-1);
		font-size: 13px;
		color: var(--muted);
		white-space: nowrap;
	}

	.split {
		display: grid;
		gap: var(--space-4);
		align-items: start;
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
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg);
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
		color: var(--muted);
	}

	.note {
		font-size: 12px;
		color: var(--danger);
	}

	.empty {
		margin: 0;
		padding: var(--space-3) 0;
		color: var(--muted);
		font-size: 13px;
		text-align: center;
	}
</style>
