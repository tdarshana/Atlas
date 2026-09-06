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
	import { Button, Checkbox, Typeahead } from '$lib/ds';
	import KindIcon from '$lib/components/board/KindIcon.svelte';
	import { groupAssignees } from '$lib/shell/sidepanels/boardFilters';
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
		backTask,
		startBoardPolling,
		refresh,
		reload,
		scheduleRefresh,
		setDetailWidth,
		setLaneWidth,
		toggleDetailMode,
		toggleLaneCollapsed,
		visibleLanes
	} from '$lib/stores/board.svelte';
	import { followPersonaChanges, loadRoster, personas } from '$lib/stores/personas.svelte';
	import { setHeaderActions } from '$lib/stores/project.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import type { Stage, Task } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	/** `global` is the every-project board, which the daemon means by no `project_id`. */
	const routeId = $derived(page.params.id ?? 'global');
	const projectId = $derived(routeId === 'global' ? null : routeId);
	const project = $derived(projects.items.find((p) => p.id === projectId) ?? null);
	const name = $derived(projectId ? (project?.name ?? 'Project') : 'Global');

	// The filters panel sets a column, an unassigned toggle and a persona; all narrow what
	// is drawn rather than what is fetched, so the panel's counts stay whole.
	const byPersona = $derived(
		board.filters.persona
			? board.tasks.filter((t) => t.persona_slug === board.filters.persona)
			: board.tasks
	);
	const shown = $derived(
		board.filters.unassigned ? byPersona.filter((t) => !t.assignee) : byPersona
	);
	const lanes = $derived(
		visibleLanes(deriveColumns(board.stages, shown), board.filters.stage, board.collapsedLanes)
	);
	const stageOptions = $derived(board.stages.map((s) => ({ value: s.name, label: s.name })));

	/** Suggestions under the search box: the project's tasks matching the text (the
	 * daemon matches key, title and description), newest first, at most eight; picking
	 * one opens it and leaves the text as the board's filter. */
	let searchHits = $state<Task[]>([]);
	let searchTimer: ReturnType<typeof setTimeout> | undefined;
	let searchGeneration = 0;
	async function suggestTasks(): Promise<void> {
		const g = ++searchGeneration;
		const q = board.filters.query.trim();
		if (q === '') {
			searchHits = [];
			return;
		}
		try {
			const hits = await api().listTasks({
				project_id: board.filters.projectId,
				query: q,
				include_done: !board.filters.hideDone,
				brief: true
			});
			if (g !== searchGeneration) return;
			searchHits = [...hits]
				.sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
				.slice(0, 8);
		} catch {
			searchHits = [];
		}
	}
	function onSearchInput(): void {
		scheduleRefresh();
		clearTimeout(searchTimer);
		searchTimer = setTimeout(() => void suggestTasks(), 150);
	}
	const searchRows = $derived(searchHits.map((t) => ({ value: t.key, label: t.title, hint: t.key, kind: t.kind })));

	/** Suggestions under the assignee box: every assignee on this project's tasks (done
	 * ones included, so a name does not vanish once its work is done), matched to the
	 * text; loaded once the box is focused. Picking one sets the filter. */
	let knownAssignees = $state<string[]>([]);
	let assigneesFor: string | null | undefined;
	async function loadAssignees(): Promise<void> {
		if (assigneesFor === board.filters.projectId) return;
		assigneesFor = board.filters.projectId;
		try {
			const tasks = await api().listTasks({ project_id: board.filters.projectId, include_done: true, brief: true });
			knownAssignees = groupAssignees(tasks).named.map(([name]) => name);
		} catch {
			knownAssignees = [];
		}
	}
	const assigneeRows = $derived.by(() => {
		const q = board.filters.assignee.trim().toLowerCase();
		return knownAssignees
			.filter((n) => n.toLowerCase().includes(q) && n !== board.filters.assignee.trim())
			.slice(0, 8)
			.map((n) => ({ value: n, label: n }));
	});

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
				board.filters.persona = '';
			}
			board.filters.projectId = id;
			board.filters.query = q;
			// The persona select and chips read this project's roster.
			if (id === null) personas.roster = [];
			else void loadRoster(id);
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
		const stopPolling = startBoardPolling();
		const stopPersonas = followPersonaChanges();
		// A pending debounce would fire a request for a screen that is gone.
		return () => {
			cancelRefresh();
			stopPolling();
			stopPersonas();
		};
	});
</script>

{#snippet headerActions()}
	<!-- Filters read left to right at the start of the row; the one action sits at the end. -->
	<div class="search">
		<Typeahead
			bind:value={board.filters.query}
			rows={searchRows}
			data-testid="board-search"
			aria-label="Search tasks"
			placeholder="Search tasks..."
			oninput={onSearchInput}
			onpick={(r) => openTask(r.value)}
		>
			{#snippet row(r)}
				<KindIcon kind={r.kind} size={14} />
				<code class="hit-key">{r.value}</code>
				<span class="dbm-menu__label">{r.label}</span>
			{/snippet}
		</Typeahead>
	</div>
	<div class="assignee">
		<Typeahead
			bind:value={board.filters.assignee}
			rows={assigneeRows}
			data-testid="board-assignee"
			aria-label="Assignee"
			placeholder="Assignee"
			oninput={() => scheduleRefresh()}
			onfocus={() => void loadAssignees()}
			onpick={(r) => {
				board.filters.assignee = r.value;
				scheduleRefresh(0);
			}}
		/>
	</div>
	<Checkbox
		label="Hide done"
		bind:checked={board.filters.hideDone}
		data-testid="board-hide-done"
		onchange={() => scheduleRefresh(0)}
	/>
	<Checkbox
		label="Hide subtasks"
		bind:checked={board.filters.hideSubtasks}
		data-testid="board-hide-subtasks"
		onchange={() => scheduleRefresh(0)}
	/>
	<span class="grow"></span>
	<Button data-testid="board-new" onclick={() => (creating = true)}>
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
				onopen={openTask}
				backKey={board.detailHistory.at(-1) ?? null}
				onback={backTask}
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
	.hit-key {
		flex: none;
		font-family: var(--font-mono);
		font-size: var(--mono-sm);
		color: var(--text-secondary);
	}
</style>
