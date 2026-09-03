<script lang="ts">
	// Edits a stage list. Renaming a stage has to move the tasks standing in it, and
	// the daemon can only do that if it is told which old name became which new one,
	// so each row remembers the name it arrived with.
	import { untrack } from 'svelte';
	import { stageRenames, validateStages } from '$lib/stores/board.svelte';
	import type { Stage } from '$lib/types';
	import Button from '$lib/ui/Button.svelte';
	import Input from '$lib/ui/Input.svelte';

	interface Props {
		stages: Stage[];
		onsave: (stages: Stage[], renames: Record<string, string>) => void | Promise<void>;
	}

	let { stages, onsave }: Props = $props();

	interface Row {
		id: number;
		name: string;
		done: boolean;
		/** The name the server has for this row, or null for a row added here. */
		original: string | null;
	}

	let nextId = 0;
	let rows = $state<Row[]>([]);
	let saving = $state(false);
	let dirty = $state(false);

	/** The list the rows were last built from, so an identical prop is not a change. */
	let applied: string | null = null;

	const signature = (list: Stage[]) => JSON.stringify(list.map((s) => [s.name, s.done]));

	// The server's list seeds the draft. A parent that hands back the same list, which
	// is what the Settings page's Reload does when nothing changed, is not a change and
	// must not wipe half-typed names; neither is any prop change while the draft is
	// dirty, since unsaved work outranks a list that moved underneath it.
	$effect(() => {
		const next = signature(stages);
		if (next === applied) return;
		const list = stages.map((s) => ({ ...s }));
		untrack(() => {
			if (dirty) return;
			applied = next;
			rows = list.map((s) => ({ id: nextId++, name: s.name, done: s.done, original: s.name }));
		});
	});

	const draft = $derived(rows.map((r) => ({ name: r.name.trim(), done: r.done })));
	const problem = $derived(validateStages(draft));

	function add() {
		rows.push({ id: nextId++, name: '', done: false, original: null });
		dirty = true;
	}

	function remove(index: number) {
		rows.splice(index, 1);
		dirty = true;
	}

	function swap(index: number, delta: number) {
		const to = index + delta;
		if (to < 0 || to >= rows.length) return;
		const [row] = rows.splice(index, 1);
		rows.splice(to, 0, row);
		dirty = true;
	}

	async function save() {
		if (problem) return;
		saving = true;
		// Cleared before the call so the saved list can flow back in through the effect.
		// A refusal leaves the parent's `stages` untouched, so the draft survives it.
		dirty = false;
		try {
			await onsave(draft, stageRenames(rows));
		} finally {
			saving = false;
		}
	}
</script>

<div class="editor" data-testid="stage-editor">
	<ul class="rows">
		{#each rows as row, i (row.id)}
			<li>
				<Input
					bind:value={row.name}
					aria-label="Stage {i + 1} name"
					data-testid="stage-name-{i}"
					oninput={() => (dirty = true)}
				/>
				<label class="done">
					<input
						type="checkbox"
						bind:checked={row.done}
						data-testid="stage-done-{i}"
						onchange={() => (dirty = true)}
					/>
					<span>Done</span>
				</label>
				<Button
					size="sm"
					aria-label="Move stage {i + 1} up"
					disabled={i === 0}
					onclick={() => swap(i, -1)}
				>
					↑
				</Button>
				<Button
					size="sm"
					aria-label="Move stage {i + 1} down"
					disabled={i === rows.length - 1}
					onclick={() => swap(i, 1)}
				>
					↓
				</Button>
				<Button
					size="sm"
					variant="ghost"
					aria-label="Remove stage {i + 1}"
					data-testid="stage-remove-{i}"
					onclick={() => remove(i)}
				>
					×
				</Button>
			</li>
		{/each}
	</ul>

	{#if problem}
		<p class="bad" role="alert" data-testid="stage-error">{problem}</p>
	{/if}

	<div class="foot">
		<Button size="sm" data-testid="stage-add" onclick={add}>Add stage</Button>
		<Button
			size="sm"
			variant="primary"
			data-testid="stage-save"
			disabled={saving || !!problem}
			onclick={save}
		>
			{saving ? 'Saving…' : 'Save stages'}
		</Button>
	</div>
</div>

<style>
	.editor {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
	}

	.rows {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.rows li {
		display: grid;
		grid-template-columns: 1fr auto auto auto auto;
		align-items: center;
		gap: var(--space-2);
	}

	.done {
		display: flex;
		align-items: center;
		gap: var(--space-1);
		font-size: 13px;
		color: var(--muted);
		white-space: nowrap;
	}

	.foot {
		display: flex;
		gap: var(--space-2);
	}

	.bad {
		margin: 0;
		color: var(--danger);
	}
</style>
