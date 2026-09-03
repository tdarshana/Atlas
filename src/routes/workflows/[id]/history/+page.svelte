<script lang="ts">
	// The Run history tab (frame 08.1): the runs table on the left, the selected run's
	// detail on the right. `?run=<id>` preselects, which is how the editor's Run action
	// and the side panel's RUNS rows land here on a specific run.
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { setStatusItems } from '$lib/shell';
	import { loadRunHistory, workflow } from '$lib/stores/workflows.svelte';
	import type { WorkflowRun } from '$lib/types';
	import RunDetail from '$lib/components/workflow/RunDetail.svelte';
	import RunList from '$lib/components/workflow/RunList.svelte';

	let selectedRunId = $state<string | null>(null);

	$effect(() => {
		if (workflow.current) void loadRunHistory();
	});

	// `?run=<id>` preselects without fighting a click made after the page loaded.
	$effect(() => {
		const wanted = page.url.searchParams.get('run');
		untrack(() => {
			if (wanted && selectedRunId !== wanted) selectedRunId = wanted;
		});
	});

	$effect(() => {
		const total = workflow.history.length;
		const ok = workflow.history.filter((r) => r.status === 'success').length;
		const failed = workflow.history.filter((r) => r.status === 'failed').length;
		setStatusItems({ right: total ? [{ text: `${total} runs · ${ok} ok · ${failed} failed` }] : [] });
	});

	function selectRun(id: string): void {
		selectedRunId = id;
		const current = workflow.current;
		if (current) void goto(`/workflows/${current.id}/history?run=${id}`, { replaceState: true, keepFocus: true, noScroll: true });
	}

	function onRerun(created: WorkflowRun): void {
		selectRun(created.id);
	}
</script>

<div class="history">
	<RunList runs={workflow.history} selectedId={selectedRunId} onSelect={selectRun} />
	<RunDetail runId={selectedRunId} {onRerun} />
</div>

<style>
	.history {
		flex: 1;
		display: flex;
		gap: 12px;
		min-height: 0;
	}
</style>
