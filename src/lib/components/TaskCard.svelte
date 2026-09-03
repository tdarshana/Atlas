<script lang="ts">
	// One card in a board column. The card is a plain container: the key and title are
	// a real button, so the keyboard reaches them natively, and the move select sits
	// outside that button because ARIA forbids interactive descendants of one.
	import type { Stage, Task } from '$lib/types';
	import Badge from '$lib/ui/Badge.svelte';
	import Select from '$lib/ui/Select.svelte';

	interface Props {
		task: Task;
		stages: Stage[];
		onopen: (key: string) => void;
		onmove: (key: string, stage: string) => void;
	}

	let { task, stages, onopen, onmove }: Props = $props();

	// A task can sit in a stage the board no longer has. With no option of its own the
	// browser would show the first stage instead, contradicting the column's own note.
	const stageOptions = $derived(
		stages.some((s) => s.name === task.stage)
			? stages.map((s) => ({ value: s.name, label: s.name }))
			: [
					{ value: task.stage, label: `${task.stage} (removed)` },
					...stages.map((s) => ({ value: s.name, label: s.name }))
				]
	);

	// Only the blockers that are not themselves done, which is what the ready rule
	// counts. Counting every link would put a red badge on a task an agent is free to
	// claim, and the drawer, which reads `ready`, would disagree with the card.
	const blockers = $derived(task.open_blockers);
</script>

<div class="card" data-testid="task-card-{task.key}">
	<button
		type="button"
		class="open"
		data-testid="task-open-{task.key}"
		onclick={() => onopen(task.key)}
	>
		<span class="top">
			<code class="key">{task.key}</code>
			<span
				class="dot {task.priority}"
				title="Priority: {task.priority}"
				aria-label="Priority: {task.priority}"
			></span>
		</span>

		<span class="title">{task.title}</span>

		<span class="meta">
			<Badge>{task.kind}</Badge>
			{#if task.assignee}
				<Badge tone="accent">{task.assignee}</Badge>
			{/if}
			{#if blockers > 0}
				<Badge tone="danger" title="Waiting on {blockers} task{blockers === 1 ? '' : 's'}">
					{blockers} blocked by
				</Badge>
			{/if}
		</span>
	</button>

	<Select
		value={task.stage}
		options={stageOptions}
		class="move"
		aria-label="Move {task.key}"
		data-testid="task-move-{task.key}"
		onchange={(e: Event & { currentTarget: HTMLSelectElement }) =>
			onmove(task.key, e.currentTarget.value)}
	/>
</div>

<style>
	.card {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: var(--space-3);
		border: 1px solid var(--border-default);
		border-radius: var(--radius-md);
		background: var(--bg-raised);
	}

	.card:hover {
		background: var(--bg-hover);
	}

	.open {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		padding: 0;
		border: none;
		background: none;
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: pointer;
	}

	.open:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: 2px;
	}

	.top {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-2);
	}

	.key {
		font-size: 12px;
		color: var(--text-secondary);
	}

	.dot {
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: var(--text-secondary);
		flex: none;
	}

	.dot.low {
		background: var(--border-default);
	}

	.dot.high {
		background: var(--accent);
	}

	.dot.urgent {
		background: var(--danger);
	}

	.title {
		overflow-wrap: anywhere;
	}

	.meta {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-1);
	}

	.card :global(select.move) {
		font-size: 13px;
		padding: 3px 8px;
	}
</style>
