<script lang="ts">
	// The 300px right panel (frame 08): the selected node's own fields, kept local to
	// the graph store until Save. A trigger and the output edit their fixed handful of
	// fields; an action gets the full form plus Practices and Source memories.
	import { onMount } from 'svelte';
	import { Badge, Button, Checkbox, Icon, Input, Select, type IconName } from '$lib/ds';
	import { personas, loadPersonas } from '$lib/stores/personas.svelte';
	import { practices } from '$lib/stores/docs.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import {
		cronHint,
		isActionData,
		isOutputData,
		isTriggerData,
		removeNode,
		save,
		updateNodeData,
		workflow
	} from '$lib/stores/workflows.svelte';
	import type { MemoryKind, MemorySource } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/platform/toasts.svelte';
	import { errorMessage } from '$lib/errors';

	const RAW_AGENTS = ['cli/claude-code', 'cli/codex', 'desktop'];
	const MEMORY_KINDS: MemoryKind[] = ['fact', 'decision', 'preference', 'insight', 'todo'];
	/** `Select` binds a string; the global scope is the empty value, as `DocEditor` does. */
	const GLOBAL = '';

	let confirming = $state(false);

	onMount(() => {
		if (!personas.items.length && !personas.loading) void loadPersonas();
		if (!practices.state.loaded) void practices.load();
		if (!projects.items.length) void loadProjects();
	});

	const node = $derived(workflow.graph.nodes.find((n) => n.id === workflow.selectedNodeId) ?? null);
	const index = $derived(workflow.graph.nodes.findIndex((n) => n.id === workflow.selectedNodeId));

	const kindLabel = $derived(
		node?.type === 'trigger' ? 'Trigger' : node?.type === 'output' ? 'Output' : 'Action'
	);
	const kindIcon = $derived<IconName>(
		node?.type === 'trigger' ? 'zap' : node?.type === 'output' ? 'list-checks' : 'play'
	);

	const agentOptions = $derived.by(() => {
		const seen = new Set<string>();
		const options: { value: string; label: string }[] = [];
		for (const a of [...RAW_AGENTS, ...personas.items.map((a) => a.slug)]) {
			if (seen.has(a)) continue;
			seen.add(a);
			options.push({ value: a, label: a });
		}
		return options;
	});

	const availablePractices = $derived.by(() => {
		if (!node || !isActionData(node.data)) return [];
		const attached = new Set(node.data.practices);
		return practices.state.list.filter((p) => !attached.has(p.name));
	});
	let practiceToAdd = $state('');

	const projectOptions = $derived([
		{ value: GLOBAL, label: 'Global (no project)' },
		...projects.items.map((p) => ({ value: p.id, label: p.name }))
	]);

	function addPractice(): void {
		if (!node || !isActionData(node.data) || !practiceToAdd) return;
		updateNodeData(node.id, { practices: [...node.data.practices, practiceToAdd] });
		practiceToAdd = '';
	}

	function removePractice(name: string): void {
		if (!node || !isActionData(node.data)) return;
		updateNodeData(node.id, { practices: node.data.practices.filter((p) => p !== name) });
	}

	function toggleMemories(on: boolean): void {
		if (!node) return;
		const empty: MemorySource = { kinds: [], tags: [], limit: 20, project_id: null };
		updateNodeData(node.id, { memories: on ? empty : null });
	}

	function toggleKind(kind: MemoryKind): void {
		if (!node || !isActionData(node.data) || !node.data.memories) return;
		const kinds = node.data.memories.kinds.includes(kind)
			? node.data.memories.kinds.filter((k) => k !== kind)
			: [...node.data.memories.kinds, kind];
		updateNodeData(node.id, { memories: { ...node.data.memories, kinds } });
	}

	function setMemoriesField(patch: Partial<MemorySource>): void {
		if (!node || !isActionData(node.data) || !node.data.memories) return;
		updateNodeData(node.id, { memories: { ...node.data.memories, ...patch } });
	}

	async function handleSave(): Promise<void> {
		try {
			await save();
			push('success', 'Saved');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	function confirmRemove(): void {
		if (!node) return;
		confirming = false;
		removeNode(node.id);
	}
</script>

{#if node}
	<div class="inspector" data-testid="inspector">
		<div class="header">
			<Icon name={kindIcon} size={13} color="var(--accent)" />
			<span class="title">{kindLabel}</span>
			<span class="spacer"></span>
			<span class="mono meta">node {index + 1} of {workflow.graph.nodes.length}</span>
		</div>

		{#if workflow.saveError}
			<p class="save-error" role="alert" data-testid="workflow-save-error">{workflow.saveError}</p>
		{/if}

		<div class="body">
			{#if isActionData(node.data)}
				{@const data = node.data}
				<Input
					label="Name"
					bind:value={() => data.name, (v) => updateNodeData(node.id, { name: v })}
					data-testid="inspector-name"
				/>
				<Select
					label="Agent"
					options={agentOptions}
					bind:value={() => data.agent, (v) => updateNodeData(node.id, { agent: v })}
					data-testid="inspector-agent"
				/>
				<label class="field">
					<span>Instructions</span>
					<Textarea
						mono
						rows={6}
						bind:value={() => data.instructions, (v) => updateNodeData(node.id, { instructions: v })}
						data-testid="inspector-instructions"
					/>
				</label>

				<div class="field">
					<span class="group-heading">Practices</span>
					<div class="chips">
						{#each data.practices as name (name)}
							<Badge mono>
								{name}
								<button
									type="button"
									class="chip-x"
									aria-label={`Remove ${name}`}
									onclick={() => removePractice(name)}
								>
									<Icon name="x" size={10} />
								</button>
							</Badge>
						{/each}
					</div>
					<div class="add-row">
						<Select
							options={[{ value: '', label: 'Choose a practice' }, ...availablePractices.map((p) => ({ value: p.name, label: p.name }))]}
							bind:value={practiceToAdd}
						/>
						<Button size="sm" disabled={!practiceToAdd} onclick={addPractice}>Add</Button>
					</div>
				</div>

				<div class="field">
					<span class="group-heading">Source memories</span>
					<Checkbox
						label="Attach memories to the prompt"
						bind:checked={() => data.memories !== null, toggleMemories}
					/>
					{#if data.memories}
						{@const memories = data.memories}
						<div class="chips">
							{#each MEMORY_KINDS as kind (kind)}
								<button type="button" class="kind-chip" onclick={() => toggleKind(kind)}>
									<Badge tone={memories.kinds.includes(kind) ? 'accent' : 'neutral'} mono>
										{kind}
									</Badge>
								</button>
							{/each}
						</div>
						<div class="pair">
							<Input
								label="Tags"
								mono
								bind:value={
									() => memories.tags.join(', '),
									(v) =>
										setMemoriesField({
											tags: v
												.split(',')
												.map((s) => s.trim())
												.filter(Boolean)
										})
								}
							/>
							<Input
								label="Limit"
								type="number"
								mono
								bind:value={
									() => String(memories.limit),
									(v) => setMemoriesField({ limit: Number(v) || 20 })
								}
							/>
						</div>
						<Select
							label="Project"
							options={projectOptions}
							bind:value={
								() => memories.project_id ?? GLOBAL,
								(v) => setMemoriesField({ project_id: v === GLOBAL ? null : v })
							}
						/>
					{/if}
				</div>
			{:else if isTriggerData(node.data)}
				{@const data = node.data}
				<Select
					label="Kind"
					options={[
						{ value: 'manual', label: 'Manual only' },
						{ value: 'schedule', label: 'On schedule' },
						{ value: 'prompt', label: 'On prompt instruction' }
					]}
					bind:value={
						() => data.kind,
						(v) => updateNodeData(node.id, { kind: v as typeof data.kind })
					}
					data-testid="inspector-trigger-kind"
				/>
				{#if data.kind === 'schedule'}
					<Input
						label="Cron"
						mono
						hint={cronHint(data.cron ?? '')}
						bind:value={() => data.cron ?? '', (v) => updateNodeData(node.id, { cron: v })}
						data-testid="inspector-cron"
					/>
				{/if}
				{#if data.kind === 'prompt'}
					<label class="field">
						<span>Prompt</span>
						<Textarea
							rows={4}
							bind:value={() => data.prompt ?? '', (v) => updateNodeData(node.id, { prompt: v })}
							data-testid="inspector-prompt"
						/>
					</label>
				{/if}
			{:else if isOutputData(node.data)}
				{@const data = node.data}
				<Checkbox
					label="Propose memories to Review"
					bind:checked={
						() => data.propose_memories,
						(v) => updateNodeData(node.id, { propose_memories: v })
					}
				/>
				<Checkbox
					label="File tasks on the board"
					bind:checked={() => data.file_tasks, (v) => updateNodeData(node.id, { file_tasks: v })}
				/>
			{/if}
		</div>

		<div class="footer">
			<Button variant="primary" size="sm" disabled={workflow.saving} onclick={handleSave}>
				{workflow.saving ? 'Saving…' : 'Save'}
			</Button>
			<span title="Single step runs arrive later">
				<Button size="sm" disabled>Run step</Button>
			</span>
			<span class="spacer"></span>
			{#if isActionData(node.data)}
				<Button variant="danger" size="sm" onclick={() => (confirming = true)}>Remove…</Button>
			{/if}
		</div>
	</div>

	<Dialog open={confirming} title="Remove action" onclose={() => (confirming = false)}>
		<p>Remove this action from the graph?</p>
		{#snippet footer()}
			<Button onclick={() => (confirming = false)}>Cancel</Button>
			<Button variant="danger" onclick={confirmRemove}>Remove</Button>
		{/snippet}
	</Dialog>
{/if}

<style>
	.inspector {
		width: 300px;
		flex: 0 0 300px;
		display: flex;
		flex-direction: column;
		min-height: 0;
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		overflow: hidden;
	}

	.header {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 0 10px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.title {
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.meta {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.save-error {
		margin: 0;
		padding: 8px 10px;
		background: var(--danger-muted);
		color: var(--danger-text);
		font-size: 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.body {
		flex: 1;
		overflow: auto;
		padding: 10px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.field > span:first-child {
		color: var(--text-secondary);
	}

	.pair {
		display: grid;
		grid-template-columns: 1fr 80px;
		gap: 8px;
	}

	.chips {
		display: flex;
		gap: 4px;
		flex-wrap: wrap;
	}

	.chip-x {
		display: inline-flex;
		align-items: center;
		margin-left: 4px;
		border: 0;
		background: transparent;
		color: inherit;
		cursor: default;
	}

	.kind-chip {
		border: 0;
		background: transparent;
		padding: 0;
		cursor: default;
	}

	.add-row {
		display: flex;
		gap: 4px;
	}

	.add-row :global(.dbm-select-wrap) {
		flex: 1;
	}

	.footer {
		height: 40px;
		flex: 0 0 40px;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 10px;
		border-top: 1px solid var(--border-subtle);
	}
</style>
