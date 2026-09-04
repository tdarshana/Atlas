<script lang="ts">
	// The Run history tab's right card (frame 08.1): the selected run's header, its
	// summary row and its per-step log. Polls `GET /runs/{id}` every 2s while the run is
	// still queued or running, so a run started from here (or from the editor) is
	// watched to completion without a manual refresh.
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { plural } from '$lib/format';
	import { inTauri } from '$lib/shell';
	import { Badge, Button } from '$lib/ds';
	import {
		cancelRun,
		loadRunDetail,
		rerun,
		RUN_STATUS_LABEL,
		RUN_STATUS_TONE,
		workflow
	} from '$lib/stores/workflows.svelte';
	import type { RunSummary, WorkflowRun } from '$lib/types';
	import { push } from '$lib/ui/toasts.svelte';
	import StepLog from './StepLog.svelte';

	const POLL_MS = 2000;

	let { runId, onRerun }: { runId: string | null; onRerun?: (run: WorkflowRun) => void } =
		$props();

	let exporting = $state(false);

	$effect(() => {
		if (runId) void loadRunDetail(runId);
		else workflow.runDetail = null;
	});

	// Re-runs whenever `runId` or the run's own status changes; only ever holds one
	// interval, since the returned cleanup clears it before the effect runs again.
	$effect(() => {
		const id = runId;
		const status = workflow.runDetail?.run.status;
		if (!id || (status !== 'queued' && status !== 'running')) return;
		const handle = setInterval(() => void loadRunDetail(id), POLL_MS);
		return () => clearInterval(handle);
	});

	const run = $derived(workflow.runDetail?.run ?? null);
	const steps = $derived(workflow.runDetail?.steps ?? []);
	const summary = $derived(run?.summary as RunSummary | null | undefined);
	const stepsCount = $derived(summary?.steps ?? steps.length);
	const memoriesProposed = $derived(summary?.memories_proposed ?? 0);
	const tasksFiled = $derived(summary?.tasks_filed ?? 0);
	const tokens = $derived(
		summary?.tokens == null ? 'n/a' : summary.tokens.toLocaleString()
	);
	const cancellable = $derived(run?.status === 'queued' || run?.status === 'running');

	async function doRerun(): Promise<void> {
		if (!run) return;
		try {
			const created = await rerun(run);
			push('success', `Run ${created.number} queued`);
			onRerun?.(created);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	async function doCancel(): Promise<void> {
		if (!run) return;
		try {
			await cancelRun(run.id);
			push('success', 'Run cancelled');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	function download(filename: string, text: string): void {
		const url = URL.createObjectURL(new Blob([text], { type: 'text/plain' }));
		const a = document.createElement('a');
		a.href = url;
		a.download = filename;
		a.click();
		URL.revokeObjectURL(url);
	}

	async function exportLog(): Promise<void> {
		if (!run || exporting) return;
		exporting = true;
		try {
			const text = await api().runExport(run.id);
			const filename = `workflow-run-${run.number}.log`;
			if (inTauri()) {
				const { save } = await import('@tauri-apps/plugin-dialog');
				// The app may only write into Downloads, Documents and the Desktop, so the
				// dialog opens where the export can actually land.
				const { downloadDir } = await import('@tauri-apps/api/path');
				const dir = await downloadDir().catch(() => null);
				const path = await save({ defaultPath: dir ? `${dir}/${filename}` : filename });
				if (!path) return;
				const { writeTextFile } = await import('@tauri-apps/plugin-fs');
				await writeTextFile(path, text);
				push('success', `Log written to ${path}`);
			} else {
				download(filename, text);
				push('success', `Exported ${filename}`);
			}
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			exporting = false;
		}
	}
</script>

<div class="card">
	{#if !run}
		<div class="empty">
			<span>Select a run</span>
		</div>
	{:else}
		<div class="head">
			<span class="mono run-name">Run {run.number}</span>
			<Badge tone={RUN_STATUS_TONE[run.status]}>{RUN_STATUS_LABEL[run.status]}</Badge>
			<span class="spacer"></span>
			<Button variant="primary" size="sm" onclick={doRerun} data-testid="run-rerun">
				Re-run
			</Button>
			<Button size="sm" disabled={exporting} onclick={exportLog} data-testid="run-export">
				{exporting ? 'Exporting…' : 'Export log'}
			</Button>
			{#if cancellable}
				<Button variant="ghost" size="sm" onclick={doCancel} data-testid="run-cancel">
					Cancel
				</Button>
			{/if}
		</div>

		<div class="summary">
			<span class="mono">{plural(stepsCount, 'step')}</span>
			<span class="mono">{memoriesProposed} memories proposed</span>
			<span class="mono">{tasksFiled} tasks filed</span>
			<span class="mono">tokens {tokens}</span>
			<span class="spacer"></span>
			<span class="trigger">trigger <span class="mono">{run.trigger}</span></span>
		</div>

		<div class="steps">
			{#if steps.length === 0}
				<p class="hint">No steps yet.</p>
			{:else}
				{#each steps as step (step.id)}
					<StepLog {step} />
				{/each}
			{/if}
		</div>
	{/if}
</div>

<style>
	.card {
		flex: 1;
		min-width: 0;
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		overflow: hidden;
		display: flex;
		flex-direction: column;
		min-height: 0;
	}

	.empty {
		flex: 1;
		display: flex;
		align-items: center;
		justify-content: center;
		color: var(--text-secondary);
	}

	.head {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.run-name {
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.summary {
		height: 28px;
		flex: 0 0 28px;
		display: flex;
		align-items: center;
		gap: 16px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
		color: var(--text-secondary);
		font-size: 12px;
	}

	.trigger {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.steps {
		flex: 1;
		overflow: auto;
	}

	.hint {
		margin: 0;
		padding: 12px;
		color: var(--text-secondary);
	}
</style>
