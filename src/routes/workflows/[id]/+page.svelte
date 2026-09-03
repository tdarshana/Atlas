<script lang="ts">
	// The Editor tab (frame 08): the canvas on the left, the 300px inspector on the right
	// for whichever node is selected.
	import Inspector from '$lib/components/workflow/Inspector.svelte';
	import WorkflowCanvas from '$lib/components/workflow/WorkflowCanvas.svelte';
	import { setStatusItems } from '$lib/shell';
	import { statusLine, workflow } from '$lib/stores/workflows.svelte';

	$effect(() => {
		const line = statusLine();
		setStatusItems({ right: line ? [{ text: line }] : [] });
	});
</script>

<div class="editor">
	{#if workflow.error}
		<p class="error" role="alert">{workflow.error}</p>
	{:else}
		<WorkflowCanvas />
		<Inspector />
	{/if}
</div>

<style>
	.editor {
		flex: 1;
		display: flex;
		gap: 12px;
		min-height: 0;
	}

	.error {
		margin: auto;
		color: var(--danger-text);
	}
</style>
