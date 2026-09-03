<script lang="ts">
	// `/workflows` with nothing picked: the frame's empty state. The list itself lives in
	// the side panel; `/workflows/[id]` is the editor for whichever one is open.
	import { goto } from '$app/navigation';
	import { Button } from '$lib/ds';
	import { createWorkflow } from '$lib/stores/workflows.svelte';
	import { errorMessage } from '$lib/errors';
	import { push } from '$lib/ui/toasts.svelte';

	async function newWorkflow(): Promise<void> {
		try {
			const created = await createWorkflow();
			await goto(`/workflows/${created.id}`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

<div class="empty">
	<p class="title">No workflow selected</p>
	<Button variant="primary" onclick={newWorkflow}>New workflow</Button>
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
