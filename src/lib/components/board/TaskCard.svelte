<script lang="ts">
	// One task in a lane, per frame 02.2: its key and priority dot on one line, the title,
	// then what it is and who has it, and a small select that moves it to another stage.
	// The card itself is the click target; the select is not, or moving a card would open
	// it at the same time. A subtask is a card like any other, with its parent named on a
	// line above the title the way Jira draws one; that line opens the parent instead.
	import { Badge, Icon, Select } from '$lib/ds';
	import type { SelectOption } from '$lib/ds';
	import { personaRole } from '$lib/stores/personas.svelte';
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

	// Selection can come from somewhere other than a click on this card (a subtask row in
	// the detail, `?task=` in the URL), so the selected card brings itself on screen.
	let el: HTMLDivElement | undefined = $state();
	$effect(() => {
		if (selected && el && typeof el.scrollIntoView === 'function') {
			el.scrollIntoView({ block: 'nearest' });
		}
	});

	function activate(event: KeyboardEvent) {
		if (event.key !== 'Enter' && event.key !== ' ') return;
		event.preventDefault();
		onopen(task.key);
	}

	function openParent(event: Event) {
		event.stopPropagation();
		if (task.parent_key) onopen(task.parent_key);
	}
</script>

<!-- A row in a grid is the pattern the Table uses, so a card carries the same roles. -->
<div
	bind:this={el}
	class="card"
	class:selected
	class:subtask={task.parent_key !== null}
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

	{#if task.parent_key}
		<button
			type="button"
			class="parent"
			data-testid="task-parent-{task.key}"
			title="Subtask of {task.parent_key}: {task.parent_title ?? ''}"
			onclick={openParent}
			onkeydown={(e) => e.stopPropagation()}
		>
			<Icon name="corner-down-right" size={12} />
			<span class="parent-key">{task.parent_key}</span>
			{#if task.parent_title}<span class="parent-title">{task.parent_title}</span>{/if}
		</button>
	{/if}

	<div class="title">{task.title}</div>

	<div class="tags">
		<Badge mono>{task.kind}</Badge>
		{#if task.persona_slug}
			<Badge tone="accent" icon="users" data-testid="persona-chip-{task.key}">
				{personaRole(task.persona_slug, task.persona_name ?? task.persona_slug)}
			</Badge>
		{/if}
		{#if task.subtasks_total > 0}
			<Badge mono icon="list-checks">{task.subtasks_done}/{task.subtasks_total}</Badge>
		{/if}
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

	/* A 1px ring on the border's own edge, matching the selected border rather than the
	   thick outward ring: the card can sit flush against the lane's scroll box (and does
	   after the modal detail closes and hands focus back to it), where an outward ring
	   is clipped on three sides. */
	.card:focus-visible {
		outline: 1px solid var(--focus-ring);
		outline-offset: -1px;
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

	/* A subtask reads as one at a glance: a thin accent edge and the parent line. */
	.card.subtask {
		border-left: 2px solid var(--accent);
	}

	.parent {
		display: flex;
		align-items: center;
		gap: 4px;
		min-width: 0;
		padding: 0;
		border: 0;
		background: none;
		color: var(--text-tertiary);
		font: inherit;
		font-size: 11px;
		line-height: 14px;
		cursor: pointer;
		text-align: left;
	}

	.parent:hover .parent-key,
	.parent:hover .parent-title {
		color: var(--text-secondary);
	}

	.parent:focus-visible {
		outline: var(--focus-ring-width) solid var(--focus-ring);
		outline-offset: 1px;
	}

	.parent-key {
		font-family: var(--font-mono);
		flex: 0 0 auto;
	}

	.parent-title {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.tags {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}
</style>
