<script lang="ts">
	// The project's Board tab per frames 02.2 and 02.2b: a lane per stage in a strip that
	// scrolls sideways, each lane draggable between 220 and 520px, and the open task
	// docked at the right of that strip. The project comes from the route, so there is no
	// project picker; `/projects/global/board` is the same screen with no project filter.
	//
	// The layout owns the header and the tabs, so this page lends the header its actions
	// and the side panel its filters rather than drawing either itself.
	import { onMount, tick, untrack } from 'svelte';
	import { replaceState } from '$app/navigation';
	import { page } from '$app/state';
	import NewTaskDialog from '$lib/components/NewTaskDialog.svelte';
	import StageEditor from '$lib/components/StageEditor.svelte';
	import LaneStrip from '$lib/components/board/LaneStrip.svelte';
	import TaskDetail from '$lib/components/board/TaskDetail.svelte';
	import { api } from '$lib/daemon.svelte';
	import { Button, Checkbox, Input } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { plural } from '$lib/format';
	import { clearSidePanelOverride, setSidePanelOverride, setStatusItems } from '$lib/shell';
	import BoardFilters from '$lib/shell/sidepanels/BoardFilters.svelte';
	import {
		board,
		cancelRefresh,
		closeTask,
		deriveColumns,
		loadLayout,
		move,
		openTask,
		refresh,
		reload,
		scheduleRefresh,
		setDetailWidth,
		setLaneWidth,
		toggleDetailMode,
		toggleLaneCollapsed,
		visibleLanes
	} from '$lib/stores/board.svelte';
	import { setHeaderActions } from '$lib/stores/project.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import type { Stage } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	/** `global` is the every-project board, which the daemon means by no `project_id`. */
	const routeId = $derived(page.params.id ?? 'global');
	const projectId = $derived(routeId === 'global' ? null : routeId);
	const project = $derived(projects.items.find((p) => p.id === projectId) ?? null);
	const name = $derived(projectId ? (project?.name ?? 'Project') : 'Global');

	// The filters panel sets a column and an unassigned toggle; both narrow what is drawn
	// rather than what is fetched, so the panel's counts stay whole.
	const shown = $derived(
		board.filters.unassigned ? board.tasks.filter((t) => !t.assignee) : board.tasks
	);
	const lanes = $derived(
		visibleLanes(deriveColumns(board.stages, shown), board.filters.stage, board.collapsedLanes)
	);
	const stageOptions = $derived(board.stages.map((s) => ({ value: s.name, label: s.name })));

	/** The frame's status line counts the testing column, when the board has one. */
	const testing = $derived(board.stages.find((s) => s.name.toLowerCase() === 'testing') ?? null);

	let creating = $state(false);
	let editingColumns = $state(false);

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
			// The column and unassigned filters live in module state, so another project's
			// board would otherwise open already narrowed with only the side panel saying so.
			if (board.filters.projectId !== id) {
				board.filters.stage = null;
				board.filters.unassigned = false;
			}
			board.filters.projectId = id;
			board.filters.query = q;
			// Re-read on every URL change, not only when the project does: the widths for a
			// board are the same values each time, so a `?task=` change costs one read of
			// two storage keys and moves nothing on the screen.
			loadLayout(id);
			closeTask();
			void refresh();
		});
	});

	// `?task=<key>` opens the detail on that task, whether or not the list holds it.
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

	// The Board tab lends the side panel its own body for as long as it is open.
	$effect(() => {
		setSidePanelOverride({ title: 'Board filters', component: BoardFilters });
		return clearSidePanelOverride;
	});

	// And the header its actions, so the buttons keep the state they act on.
	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});

	$effect(() => {
		const tasks = plural(board.tasks.length, 'task');
		if (!testing) {
			setStatusItems({ right: [{ text: tasks }] });
			return;
		}
		const held = board.tasks.filter((t) => t.stage === testing.name).length;
		setStatusItems({ right: [{ text: `${tasks} · ${held} in testing` }] });
	});

	/** Closes the detail and hands focus back to the card it came from. */
	async function dismiss() {
		const key = board.selected;
		closeTask();
		// `?task=` is re-read on every URL change, so a dismissed task would dock again the
		// moment anything else touched the URL. Strip it rather than leave it lying there.
		if (page.url.searchParams.has('task')) {
			const url = new URL(page.url);
			url.searchParams.delete('task');
			replaceState(url, page.state);
		}
		if (!key) return;
		// The focused close button unmounts with the panel, which would drop focus on
		// the body and restart the tab order at the top of the page.
		await tick();
		document.querySelector<HTMLElement>(`[data-testid="task-open-${key}"]`)?.focus();
	}

	/**
	 * `Add column` edits the list this board is actually drawn from: the project's own
	 * when it is on one, the global list on the every-project board. Saving a project's
	 * list gives it an override, which is what asking for a column here means.
	 */
	async function saveColumns(next: Stage[], renames: Record<string, string>) {
		try {
			if (projectId) await api().setProjectStages(projectId, next, renames);
			else await api().setBoardStages(next, renames);
			editingColumns = false;
			await refresh();
			push('success', 'Board columns saved');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	onMount(() => {
		void loadProjects();
		// A pending debounce would fire a request for a screen that is gone.
		return cancelRefresh;
	});
</script>

{#snippet headerActions()}
	<!-- Filters read left to right at the start of the row; the one action sits at the end. -->
	<div class="search">
		<Input
			bind:value={board.filters.query}
			data-testid="board-search"
			aria-label="Search tasks"
			placeholder="Search tasks..."
			oninput={() => scheduleRefresh()}
		/>
	</div>
	<div class="assignee">
		<Input
			bind:value={board.filters.assignee}
			data-testid="board-assignee"
			aria-label="Assignee"
			placeholder="Assignee"
			oninput={() => scheduleRefresh()}
		/>
	</div>
	<Checkbox
		label="Hide done"
		bind:checked={board.filters.hideDone}
		data-testid="board-hide-done"
		onchange={() => scheduleRefresh(0)}
	/>
	<span class="grow"></span>
	<Button variant="primary" data-testid="board-new" onclick={() => (creating = true)}>
		New task
	</Button>
{/snippet}

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
	<div class="dock" style="--detail-w:{board.detailWidth}px">
		<LaneStrip
			{lanes}
			widths={board.laneWidths}
			{stageOptions}
			selected={board.selected}
			onopen={openTask}
			onmove={(key, stage) => void move(key, stage)}
			onresize={setLaneWidth}
			onexpand={() => (board.filters.stage = null)}
			ontoggle={toggleLaneCollapsed}
			onaddcolumn={() => (editingColumns = true)}
		/>

		{#if board.selected}
			<TaskDetail
				detail={board.detail}
				stages={board.stages}
				loading={board.detailLoading}
				error={board.detailError}
				width={board.detailWidth}
				mode={board.detailMode}
				ontogglemode={toggleDetailMode}
				onclose={dismiss}
				onchanged={reload}
				onmove={(key, stage) => void move(key, stage)}
				onresize={setDetailWidth}
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

<Dialog
	open={editingColumns}
	title={projectId ? `Columns for ${name}` : 'Board columns'}
	onclose={() => (editingColumns = false)}
>
	<StageEditor stages={board.stages} onsave={saveColumns} />
</Dialog>

<style>
	/* The docked detail is a flex sibling of the strip, so the strip shrinks to make
	   room instead of being drawn over. */
	.dock {
		position: relative;
		display: flex;
		gap: 12px;
		flex: 1;
		min-height: 0;
	}

	/* Full-height DS inputs: the toolbar row under the tab strip has room for them. */
	.assignee {
		width: 180px;
	}

	.search {
		width: 300px;
	}

	.grow {
		flex: 1;
	}
</style>
