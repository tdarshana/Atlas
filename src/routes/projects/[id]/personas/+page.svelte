<script lang="ts">
	// The project Personas tab: this project's roster in position order, with one default
	// radio, up and down to reorder and a remove per row, plus `Add from library…`. Every
	// change writes the whole list back, the same way the Skills tab writes its disabled
	// list, so the daemon's answer is always the truth on screen.
	import { onMount } from 'svelte';
	import { Button, IconButton, Input } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import {
		followPersonaChanges,
		loadPersonas,
		loadRoster,
		personas,
		saveRoster
	} from '$lib/stores/personas.svelte';
	import { project, setHeaderActions } from '$lib/stores/project.svelte';
	import type { Persona, RosterRow } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	const id = $derived(project.current?.id ?? '');

	let adding = $state(false);
	let pickSearch = $state('');

	const rosterIds = $derived(new Set(personas.roster.map((r) => r.persona_id)));
	const available = $derived(
		personas.items.filter((p) => {
			if (rosterIds.has(p.id)) return false;
			const q = pickSearch.trim().toLowerCase();
			return !q || p.name.toLowerCase().includes(q) || p.role.toLowerCase().includes(q);
		})
	);

	type Entry = Pick<RosterRow, 'persona_id' | 'is_default'>;

	/** Writes `rows` as the whole roster, positions from their order. */
	async function write(rows: Entry[]): Promise<void> {
		if (!id) return;
		const entries = rows.map((r, i) => ({
			persona_id: r.persona_id,
			is_default: r.is_default,
			position: i
		}));
		try {
			await saveRoster(id, entries);
		} catch (e) {
			push('error', errorMessage(e));
			// The control already moved, so put the truth back on screen.
			await loadRoster(id);
		}
	}

	function move(index: number, delta: -1 | 1): void {
		const rows = [...personas.roster];
		const other = index + delta;
		if (other < 0 || other >= rows.length) return;
		[rows[index], rows[other]] = [rows[other], rows[index]];
		void write(rows);
	}

	function makeDefault(row: RosterRow): void {
		void write(
			personas.roster.map((r) => ({ ...r, is_default: r.persona_id === row.persona_id }))
		);
	}

	function remove(row: RosterRow): void {
		void write(personas.roster.filter((r) => r.persona_id !== row.persona_id));
	}

	function pick(p: Persona): void {
		// The first persona on an empty roster is the one agents adopt, so it is the default.
		const entry: Entry = { persona_id: p.id, is_default: personas.roster.length === 0 };
		closePicker();
		void write([...personas.roster, entry]);
	}

	function closePicker(): void {
		adding = false;
		pickSearch = '';
	}

	$effect(() => {
		if (!id) return;
		void loadRoster(id);
		void loadPersonas();
	});

	onMount(() => followPersonaChanges());

	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});
</script>

{#snippet headerActions()}
	<Button size="sm" data-testid="roster-add" onclick={() => (adding = true)}>
		Add from library…
	</Button>
{/snippet}

<div class="pane" data-testid="project-personas-page">
	{#if personas.rosterError}
		<p class="bad" role="alert" data-testid="roster-error">{personas.rosterError}</p>
	{/if}

	<div class="line">
		<p class="hint" data-testid="roster-explain">
			Agents on this project adopt the default persona unless a task names another; the
			roster is what atlas sync exports.
			<a href="/projects/{id}/agents" data-testid="roster-sync-link">Sync now</a>
		</p>
	</div>

	{#if personas.roster.length === 0 && !personas.rosterError}
		<EmptyState
			title="This project has no personas yet"
			hint="Use Add from library… to put one on the roster."
		>
			<Button size="sm" data-testid="roster-add-empty" onclick={() => (adding = true)}>
				Add from library…
			</Button>
		</EmptyState>
	{:else}
		<div class="table" role="grid" data-testid="roster-table">
			<div class="header-row" role="row">
				<div class="header-cell group-heading" role="columnheader">Name</div>
				<div class="header-cell group-heading" role="columnheader">Role</div>
				<div class="header-cell group-heading" role="columnheader">Default</div>
				<div class="header-cell group-heading" role="columnheader">Order</div>
				<div class="header-cell" role="columnheader"><span class="sr-only">Remove</span></div>
			</div>
			<div class="body" role="rowgroup">
				{#each personas.roster as row, i (row.persona_id)}
					<div class="row" role="row" data-testid="roster-row-{row.slug}">
						<div class="cell mono" role="gridcell">{row.name}</div>
						<div class="cell" role="gridcell">{row.role}</div>
						<div class="cell" role="gridcell">
							<input
								type="radio"
								name="roster-default"
								aria-label="Make {row.name} the default"
								checked={row.is_default}
								data-testid="roster-default-{row.slug}"
								onchange={() => makeDefault(row)}
							/>
						</div>
						<div class="cell order" role="gridcell">
							<IconButton
								icon="arrow-up"
								label="Move up"
								size="sm"
								disabled={i === 0}
								data-testid="roster-up-{row.slug}"
								onclick={() => move(i, -1)}
							/>
							<IconButton
								icon="arrow-down"
								label="Move down"
								size="sm"
								disabled={i === personas.roster.length - 1}
								data-testid="roster-down-{row.slug}"
								onclick={() => move(i, 1)}
							/>
						</div>
						<div class="cell order" role="gridcell">
							<IconButton
								icon="x"
								label="Remove from roster"
								size="sm"
								data-testid="roster-remove-{row.slug}"
								onclick={() => remove(row)}
							/>
						</div>
					</div>
				{/each}
			</div>
		</div>
	{/if}
</div>

<Dialog open={adding} title="Add from library" onclose={closePicker}>
	<div class="picker">
		<Input
			placeholder="Search by name or role"
			aria-label="Search personas"
			icon="search"
			bind:value={pickSearch}
			data-testid="roster-pick-search"
		/>
		{#if available.length === 0}
			<p class="hint" data-testid="roster-pick-empty">
				{personas.items.length > rosterIds.size
					? 'No persona matches this search.'
					: 'Every persona in the library is already on this roster.'}
			</p>
		{:else}
			<ul class="picks">
				{#each available as p (p.id)}
					<li>
						<button
							type="button"
							class="pick"
							data-testid="roster-pick-{p.slug}"
							onclick={() => pick(p)}
						>
							<span class="mono">{p.name}</span>
							<span class="role">{p.role}</span>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</Dialog>

<style>
	.pane {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.line {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
	}

	.hint {
		margin: 0;
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.hint a {
		color: var(--accent);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}

	/* The same 28px header and rows as the ds Table, without its sorting and column
	   drag: a roster is ordered by hand, so the order the store holds is the order shown. */
	.table {
		display: flex;
		flex-direction: column;
		min-height: 0;
		border: 1px solid var(--border-subtle);
		border-radius: var(--radius-md);
	}

	.header-row,
	.row {
		display: grid;
		grid-template-columns: minmax(0, 1.2fr) minmax(0, 2fr) 64px 64px 32px;
		align-items: center;
		height: 28px;
		padding: 0 12px;
		column-gap: 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.header-row {
		flex: 0 0 28px;
		user-select: none;
	}

	.header-cell {
		display: flex;
		align-items: center;
		height: 100%;
		padding: 0 8px 0 0;
		border-right: 1px solid var(--border-subtle);
	}

	.header-cell:last-child {
		border-right: 0;
		padding-right: 0;
	}

	.body {
		overflow-y: auto;
		min-height: 0;
	}

	.row:last-child {
		border-bottom: 0;
	}

	.cell {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.cell.order {
		display: flex;
		align-items: center;
		gap: 2px;
	}

	.mono {
		font-family: var(--font-mono);
		font-variant-numeric: var(--tabular);
	}

	.sr-only {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip: rect(0 0 0 0);
		white-space: nowrap;
	}

	.picker {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.picks {
		list-style: none;
		margin: 0;
		padding: 0;
		max-height: 320px;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
	}

	.pick {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		width: 100%;
		height: 28px;
		padding: 0 8px;
		border: 0;
		border-bottom: 1px solid var(--border-subtle);
		background: transparent;
		color: var(--text-primary);
		font: inherit;
		text-align: left;
		cursor: pointer;
	}

	.picks li:last-child .pick {
		border-bottom: 0;
	}

	.pick:hover {
		background: var(--bg-hover);
	}

	.pick:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: -2px;
	}

	.pick .role {
		color: var(--text-secondary);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
