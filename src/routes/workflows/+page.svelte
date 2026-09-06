<script lang="ts">
	// `/workflows` with nothing picked: the frame's empty state. The list itself lives in
	// the side panel; `/workflows/[id]` is the editor for whichever one is open.
	import { goto } from '$app/navigation';
	import { Button } from '$lib/ds';
	import { createWorkflow } from '$lib/stores/workflows.svelte';
	import { errorMessage } from '$lib/errors';
	import { push } from '$lib/platform/toasts.svelte';

	import NewWorkflowDialog from '$lib/components/workflow/NewWorkflowDialog.svelte';

	let naming = $state(false);

	async function newWorkflow(name: string): Promise<void> {
		try {
			const created = await createWorkflow(null, name);
			await goto(`/workflows/${created.id}`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

<NewWorkflowDialog open={naming} onclose={() => (naming = false)} oncreate={newWorkflow} />

<div class="empty">
	<p class="title">No workflow selected</p>
	<Button variant="primary" onclick={() => (naming = true)}>New workflow</Button>
</div>

<style>
	.empty {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: var(--space-3);
		color: var(--text-secondary);
	}

	.title {
		margin: 0;
	}
</style>
