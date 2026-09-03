<script lang="ts">
	// One task in a lane, per frame 02.2: its key and priority dot on one line, the title,
	// then what it is and who has it, and a small select that moves it to another stage.
	// The card itself is the click target; the select is not, or moving a card would open
	// it at the same time.
	import { Badge, Select } from '$lib/ds';
	import type { SelectOption } from '$lib/ds';
	import type { Task } from '$lib/types';
	import { priorityTone } from './card';

	interface Props {
		task: Task;
		/** The stages the move select offers, already in board order. */
		stageOptions: SelectOption[];
		selected: boolean;
		onopen: (key: string) => void;
		onmove: (key: string, stage: string) => void;
	}

	let { task, stageOptions, selected, onopen, onmove }: Props = $props();

	function activate(event: KeyboardEvent) {
		if (event.key !== 'Enter' && event.key !== ' ') return;
		event.preventDefault();
		onopen(task.key);
	}
</script>

<!-- A row in a grid is the pattern the Table uses, so a card carries the same roles. -->
<div
	class="card"
	class:selected
	role="button"
	tabindex="0"
	data-testid="task-open-{task.key}"
	aria-label="{task.key} {task.title}"
	onclick={() => onopen(task.key)}
	onkeydown={activate}
>
	<div class="top">
		<span class="key">{task.key}</span>
		<span class="spacer"></span>
		<span
			class="dot"
			style:background={priorityTone(task.priority)}
			title="{task.priority} priority"
		></span>
	</div>

	<div class="title">{task.title}</div>

	<div class="tags">
		<Badge mono>{task.kind}</Badge>
		{#if task.assignee}
			<Badge tone="accent" mono>{task.assignee}</Badge>
		{/if}
	</div>

	<!-- The select swallows its own clicks so moving a card does not also open it. -->
	<div
		class="move"
		role="none"
		onclick={(e) => e.stopPropagation()}
		onkeydown={(e) => e.stopPropagation()}
	>
		<Select
			size="sm"
			value={task.stage}
			options={stageOptions}
			aria-label="Move {task.key}"
			data-testid="task-move-{task.key}"
			onchange={(e: Event & { currentTarget: HTMLSelectElement }) =>
				onmove(task.key, e.currentTarget.value)}
		/>
	</div>
</div>

<style>
	.card {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 8px;
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		box-shadow: var(--shadow-sm);
		cursor: default;
	}

	.card:hover {
		border-color: var(--border-strong);
	}

	.card.selected {
		border-color: var(--accent);
	}

	.card:focus-visible {
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 1px;
	}

	.top {
		display: flex;
		align-items: center;
	}

	.key {
		font-family: var(--font-mono);
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.spacer {
		flex: 1;
	}

	.dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
	}

	.title {
		font-size: 12px;
		line-height: 16px;
		overflow-wrap: anywhere;
	}

	.tags {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}
</style>
