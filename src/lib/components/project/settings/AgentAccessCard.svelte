<script lang="ts">
	// The Agent access card of frame 02.7: a tick per actor label, plus the review rule for
	// memories an agent proposes. Everything ticked means no restriction at all, which is
	// what the daemon stores as a pair of nulls.
	//
	// The frame draws one column, and one column can only speak for one rule. When the
	// project's two rules already differ, the card draws both rather than flatten the one
	// it is not showing.
	import { Button, Checkbox, Input } from '$lib/ds';
	import { alwaysAllowed } from './settings';

	interface Props {
		/** The labels offered a tick, already sorted. */
		actors: string[];
		memoryWriters: Record<string, boolean>;
		taskMovers: Record<string, boolean>;
		requireReview: boolean;
		/** True when the loaded rules differ, so both columns are drawn. */
		split: boolean;
		/** Set once a label was added by hand, so the save writes the lists out in full. */
		manual: boolean;
	}

	let {
		actors = $bindable(),
		memoryWriters = $bindable(),
		taskMovers = $bindable(),
		requireReview = $bindable(),
		split,
		manual = $bindable()
	}: Props = $props();

	let draft = $state('');

	const trimmed = $derived(draft.trim());
	const canAdd = $derived(!!trimmed && !actors.includes(trimmed));

	/** One column drives both rules; two columns drive one each. */
	function set(actor: string, on: boolean, rule: 'memories' | 'tasks' | 'both') {
		if (rule !== 'tasks') memoryWriters = { ...memoryWriters, [actor]: on };
		if (rule !== 'memories') taskMovers = { ...taskMovers, [actor]: on };
	}

	/** A label Atlas has not seen write to this project yet, added by hand. */
	function add() {
		if (!canAdd) return;
		actors = [...actors, trimmed].sort((a, b) => a.localeCompare(b));
		set(trimmed, true, 'both');
		// A label nothing has written under is only stored if the lists are written out in
		// full; a pair of nulls would forget it the moment the save lands.
		manual = true;
		draft = '';
	}
</script>

<section class="card" data-testid="settings-access">
	<header><span class="title">Agent access</span></header>
	<div class="body">
		<span class="sentence">Agents allowed to write memories and move tasks in this project.</span>
		{#if split}
			<span class="hint">
				This project allows different agents to write memories and to move tasks, so each rule
				has its own column.
			</span>
			<div class="grid">
				<span class="group-heading"></span>
				<span class="group-heading">Memories</span>
				<span class="group-heading">Tasks</span>
				{#each actors as actor (actor)}
					<span class="mono label">
						{actor}
						{#if alwaysAllowed(actor)}<span class="always">always allowed</span>{/if}
					</span>
					<Checkbox
						aria-label="{actor} may write memories"
						checked={memoryWriters[actor] ?? false}
						onchange={(e) => set(actor, e.currentTarget.checked, 'memories')}
					/>
					<Checkbox
						aria-label="{actor} may move tasks"
						checked={taskMovers[actor] ?? false}
						onchange={(e) => set(actor, e.currentTarget.checked, 'tasks')}
					/>
				{/each}
			</div>
		{:else}
			<div class="ticks">
				{#each actors as actor (actor)}
					<div class="tick">
						<Checkbox
							label={actor}
							checked={memoryWriters[actor] ?? false}
							onchange={(e) => set(actor, e.currentTarget.checked, 'both')}
						/>
						<!-- The daemon exempts the user's own hands from both lists, so a tick
						     against one of them changes nothing; say so rather than imply a rule. -->
						{#if alwaysAllowed(actor)}<span class="always">always allowed</span>{/if}
					</div>
				{/each}
			</div>
		{/if}
		<div class="add">
			<Input
				placeholder="Add label"
				aria-label="Add label"
				mono
				bind:value={draft}
				onkeydown={(e) => {
					if (e.key === 'Enter') add();
				}}
			/>
			<Button size="sm" data-testid="access-add" disabled={!canAdd} onclick={add}>Add</Button>
		</div>
		<Checkbox
			label="Require review for memories proposed by agents"
			bind:checked={requireReview}
		/>
	</div>
</section>

<style>
	/* The pane scrolls, not the card: a card that shrank would clip the row the user
	   came here to change. */
	.card {
		display: flex;
		flex-direction: column;
		flex: 0 0 auto;
	}

	header {
		display: flex;
		align-items: center;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.title {
		font-weight: 600;
	}

	.body {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 12px;
	}

	.sentence {
		color: var(--text-secondary);
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.ticks {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.tick {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.always {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.grid {
		display: grid;
		grid-template-columns: 220px 80px 80px;
		align-items: center;
		gap: 6px;
	}

	.label {
		color: var(--text-secondary);
	}

	/* An input and a button, not a bar: the button is its own width. */
	.add {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.add :global(.dbm-input) {
		width: 220px;
	}
</style>
