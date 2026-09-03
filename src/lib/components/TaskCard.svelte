<script lang="ts">
	// One card in a board column. The whole card opens the drawer, so it is a button
	// for the keyboard too; the move select sits inside it and has to keep its own
	// clicks and keys to itself.
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

	const stageOptions = $derived(stages.map((s) => ({ value: s.name, label: s.name })));
	const blockers = $derived(task.blocked_by.length);

	/** Enter and Space do what a click does; Space would otherwise scroll the page. */
	function activate(event: KeyboardEvent) {
		if (event.key !== 'Enter' && event.key !== ' ') return;
		event.preventDefault();
		onopen(task.key);
	}
</script>

<div
	class="card"
	role="button"
	tabindex="0"
	data-testid="task-card-{task.key}"
	onclick={() => onopen(task.key)}
	onkeydown={activate}
>
	<div class="top">
		<code class="key">{task.key}</code>
		<span
			class="dot {task.priority}"
			title="Priority: {task.priority}"
			aria-label="Priority: {task.priority}"
		></span>
	</div>

	<p class="title">{task.title}</p>

	<div class="meta">
		<Badge>{task.kind}</Badge>
		{#if task.assignee}
			<Badge tone="accent">{task.assignee}</Badge>
		{/if}
		{#if blockers > 0}
			<Badge tone="danger" title="Waiting on {blockers} task{blockers === 1 ? '' : 's'}">
				{blockers} blocked by
			</Badge>
		{/if}
	</div>

	<Select
		value={task.stage}
		options={stageOptions}
		class="move"
		aria-label="Move {task.key}"
		data-testid="task-move-{task.key}"
		onclick={(e: MouseEvent) => e.stopPropagation()}
		onkeydown={(e: KeyboardEvent) => e.stopPropagation()}
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
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--bg-elev);
		cursor: pointer;
		text-align: left;
	}

	.card:hover {
		background: var(--bg-hover);
	}

	.card:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: 1px;
	}

	.top {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-2);
	}

	.key {
		font-size: 12px;
		color: var(--muted);
	}

	.dot {
		width: 8px;
		height: 8px;
		border-radius: 999px;
		background: var(--muted);
		flex: none;
	}

	.dot.low {
		background: var(--border);
	}

	.dot.high {
		background: var(--accent);
	}

	.dot.urgent {
		background: var(--danger);
	}

	.title {
		margin: 0;
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
