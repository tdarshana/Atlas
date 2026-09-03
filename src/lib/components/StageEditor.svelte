<script lang="ts">
	// Edits a stage list. Renaming a stage has to move the tasks standing in it, and
	// the daemon can only do that if it is told which old name became which new one,
	// so each row remembers the name it arrived with.
	import { validateStages } from '$lib/stores/board.svelte';
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
	let touched = $state(false);

	// The server's list is the draft's starting point, and replaces it whenever the
	// saved list comes back changed.
	$effect(() => {
		rows = stages.map((s) => ({ id: nextId++, name: s.name, done: s.done, original: s.name }));
		touched = false;
	});

	const draft = $derived(rows.map((r) => ({ name: r.name.trim(), done: r.done })));
	const problem = $derived(validateStages(draft));

	function add() {
		rows.push({ id: nextId++, name: '', done: false, original: null });
		touched = true;
	}

	function remove(index: number) {
		rows.splice(index, 1);
		touched = true;
	}

	function swap(index: number, delta: number) {
		const to = index + delta;
		if (to < 0 || to >= rows.length) return;
		const [row] = rows.splice(index, 1);
		rows.splice(to, 0, row);
		touched = true;
	}

	/** Old name to new name, for the rows whose name changed. */
	function renames(): Record<string, string> {
		const out: Record<string, string> = {};
		for (const row of rows) {
			const name = row.name.trim();
			if (row.original !== null && row.original !== name) out[row.original] = name;
		}
		return out;
	}

	async function save() {
		if (problem) return;
		saving = true;
		try {
			await onsave(draft, renames());
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
					oninput={() => (touched = true)}
				/>
				<label class="done">
					<input
						type="checkbox"
						bind:checked={row.done}
						data-testid="stage-done-{i}"
						onchange={() => (touched = true)}
					/>
					<span>Done</span>
				</label>
				<Button size="sm" aria-label="Move {row.name} up" disabled={i === 0} onclick={() => swap(i, -1)}>
					↑
				</Button>
				<Button
					size="sm"
					aria-label="Move {row.name} down"
					disabled={i === rows.length - 1}
					onclick={() => swap(i, 1)}
				>
					↓
				</Button>
				<Button
					size="sm"
					variant="ghost"
					aria-label="Remove {row.name}"
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
			disabled={saving || !!problem || !touched}
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
