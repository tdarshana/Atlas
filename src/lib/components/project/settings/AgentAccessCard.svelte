<script lang="ts">
	// The Agent access card of frame 02.7: a tick per actor label, plus the review rule for
	// memories an agent proposes. Everything ticked means no restriction at all, which is
	// what the daemon stores as a pair of nulls.
	import { Button, Checkbox, Input } from '$lib/ds';

	interface Props {
		/** The labels offered a tick, already sorted. */
		actors: string[];
		checked: Record<string, boolean>;
		requireReview: boolean;
	}

	let { actors = $bindable(), checked = $bindable(), requireReview = $bindable() }: Props =
		$props();

	let draft = $state('');

	const trimmed = $derived(draft.trim());
	const canAdd = $derived(!!trimmed && !actors.includes(trimmed));

	/** A label Atlas has not seen write to this project yet, added by hand. */
	function add() {
		if (!canAdd) return;
		actors = [...actors, trimmed].sort((a, b) => a.localeCompare(b));
		checked = { ...checked, [trimmed]: true };
		draft = '';
	}
</script>

<section class="card" data-testid="settings-access">
	<header><span class="title">Agent access</span></header>
	<div class="body">
		<span class="sentence">Agents allowed to write memories and move tasks in this project.</span>
		<div class="ticks">
			{#each actors as actor (actor)}
				<Checkbox
					label={actor}
					checked={checked[actor] ?? false}
					onchange={(e) => (checked = { ...checked, [actor]: e.currentTarget.checked })}
				/>
			{/each}
		</div>
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
			<Button data-testid="access-add" disabled={!canAdd} onclick={add}>Add label</Button>
		</div>
		<Checkbox
			label="Require review for memories proposed by agents"
			bind:checked={requireReview}
		/>
	</div>
</section>

<style>
	.card {
		display: flex;
		flex-direction: column;
		overflow: hidden;
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

	.ticks {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.add {
		display: grid;
		grid-template-columns: 220px auto;
		gap: 8px;
		align-items: center;
	}
</style>
