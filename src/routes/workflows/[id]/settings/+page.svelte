<script lang="ts">
	// The Settings tab: name, description, project and the enabled flag, then delete.
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { api } from '$lib/daemon.svelte';
	import { Button, Checkbox, Input, Select } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import { removeWorkflow, workflow } from '$lib/stores/workflows.svelte';
	import type { Workflow, WorkflowPatch } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	/** `Select` binds a string; the global scope is the empty value. */
	const GLOBAL = '';

	interface Form {
		name: string;
		description: string;
		projectId: string;
		enabled: boolean;
	}

	let form = $state<Form | null>(null);
	let loaded = $state<Form | null>(null);
	let builtFor = $state('');
	let saving = $state(false);
	let confirming = $state(false);

	const current = $derived(workflow.current);
	const dirty = $derived(!!form && JSON.stringify(form) !== JSON.stringify(loaded));

	function formFrom(w: Workflow): Form {
		return {
			name: w.name,
			description: w.description,
			projectId: w.project_id ?? GLOBAL,
			enabled: w.enabled
		};
	}

	$effect(() => {
		if (!projects.items.length) void loadProjects();
	});

	// Builds the form once per workflow; a later re-read of the same one (e.g. from the
	// layout's `openWorkflow`) leaves an in-progress edit alone.
	$effect(() => {
		const w = current;
		if (!w || w.id === builtFor) return;
		untrack(() => {
			form = formFrom(w);
			loaded = formFrom(w);
			builtFor = w.id;
		});
	});

	const projectOptions = $derived([
		{ value: GLOBAL, label: 'Global (no project)' },
		...projects.items.map((p) => ({ value: p.id, label: p.name }))
	]);

	async function save(): Promise<void> {
		const f = form;
		const w = current;
		if (!f || !w || saving) return;
		const patch: WorkflowPatch = {
			name: f.name.trim(),
			description: f.description,
			project_id: f.projectId === GLOBAL ? null : f.projectId,
			enabled: f.enabled
		};
		saving = true;
		try {
			const saved = await api().patchWorkflow(w.id, patch);
			workflow.current = saved;
			const i = workflow.list.findIndex((x) => x.id === saved.id);
			if (i >= 0) workflow.list[i] = saved;
			loaded = formFrom(saved);
			push('success', 'Saved');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			saving = false;
		}
	}

	async function confirmDelete(): Promise<void> {
		const w = current;
		confirming = false;
		if (!w) return;
		try {
			await removeWorkflow(w.id);
			await goto('/workflows');
			push('success', `Deleted ${w.name}`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

{#if form}
	<div class="stack">
		<label class="field">
			<span>Name</span>
			<Input bind:value={form.name} mono data-testid="settings-name" />
		</label>
		<label class="field">
			<span>Description</span>
			<Textarea bind:value={form.description} rows={3} data-testid="settings-description" />
		</label>
		<label class="field">
			<span>Project</span>
			<Select
				options={projectOptions}
				bind:value={form.projectId}
				data-testid="settings-project"
			/>
		</label>
		<Checkbox label="Enabled" bind:checked={form.enabled} data-testid="settings-enabled" />

		<div class="actions">
			<Button variant="primary" disabled={!dirty || saving} data-testid="settings-save" onclick={save}>
				{saving ? 'Saving…' : 'Save'}
			</Button>
			<span class="spacer"></span>
			<Button variant="danger" data-testid="settings-delete" onclick={() => (confirming = true)}>
				Delete…
			</Button>
		</div>
	</div>
{/if}

<Dialog open={confirming} title="Delete workflow" onclose={() => (confirming = false)}>
	<p>Delete <strong>{current?.name}</strong>? This cannot be undone.</p>
	{#snippet footer()}
		<Button onclick={() => (confirming = false)}>Cancel</Button>
		<Button variant="danger" data-testid="settings-delete-confirm" onclick={confirmDelete}>
			Delete
		</Button>
	{/snippet}
</Dialog>

<style>
	.stack {
		display: flex;
		flex-direction: column;
		gap: 12px;
		flex: 1;
		min-height: 0;
		max-width: 480px;
		overflow: auto;
	}

	.field {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.field > span {
		color: var(--text-secondary);
	}

	.actions {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.spacer {
		flex: 1;
	}
</style>
