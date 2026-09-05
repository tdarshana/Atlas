<script lang="ts">
	// The Agents tab's Sync card (frame 02.4): scope fixed to this project, the four
	// export targets, and the last run's plan or the empty state.
	import { Button, Checkbox, Icon, Select, Table, type TableColumn } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { DEFAULT_SYNC_TARGETS, SYNC_TARGET_LABELS, projectSyncRequest } from './sync';
	import type { SyncOp, SyncReport } from '$lib/types';
	import { skipReason } from '$lib/types';
	import { push } from '$lib/platform/toasts.svelte';

	interface Props {
		projectName: string;
		root: string;
	}

	let { projectName, root }: Props = $props();

	let choice = $state({ ...DEFAULT_SYNC_TARGETS });
	let running = $state(false);
	let report = $state<SyncReport | null>(null);
	let wasDryRun = $state(true);
	let error = $state<string | null>(null);

	const scopeOptions = $derived([{ value: 'project', label: `Project: ${projectName}` }]);
	const hasTargets = $derived(Object.values(choice).some(Boolean));

	const syncColumns: TableColumn<SyncOp>[] = [
		{ key: 'path', label: 'Path', mono: true },
		{ key: 'action', label: 'Action', width: '160px' }
	];

	async function run(checkOnly: boolean): Promise<void> {
		running = true;
		error = null;
		try {
			report = await api().sync(projectSyncRequest(root, choice, checkOnly));
			wasDryRun = checkOnly;
			if (!checkOnly) {
				const r = report;
				push('success', `Synced: ${r.created} created, ${r.updated} updated, ${r.skipped} skipped`);
			}
		} catch (e) {
			report = null;
			error = errorMessage(e);
			if (!checkOnly) push('error', error);
		} finally {
			running = false;
		}
	}

	function actionLabel(op: SyncOp): string {
		const reason = skipReason(op.action);
		return reason === null ? (op.action as string) : `Skip: ${reason}`;
	}
</script>

<div class="card">
	<div class="head">
		<span class="group-heading">Sync</span>
	</div>
	<div class="body">
		<div class="controls">
			<div class="scope">
				<Select label="Scope" value="project" options={scopeOptions} disabled data-testid="project-sync-scope" />
			</div>
			<div class="targets">
				<span class="label">Targets</span>
				<div class="checks">
					{#each SYNC_TARGET_LABELS as target (target.key)}
						<Checkbox
							label={target.label}
							checked={choice[target.key]}
							onchange={() => (choice = { ...choice, [target.key]: !choice[target.key] })}
						/>
					{/each}
				</div>
			</div>
			<span class="spacer"></span>
			<div class="buttons">
				<Button data-testid="project-sync-check" disabled={running || !hasTargets} onclick={() => run(true)}>
					Check
				</Button>
				<Button variant="primary" data-testid="project-sync-run" disabled={running || !hasTargets} onclick={() => run(false)}>
					Sync
				</Button>
			</div>
		</div>

		<p class="hint">
			A project sync writes <span class="mono">.claude/agents/</span>, <span class="mono">.codex/agents/</span>
			and managed blocks in <span class="mono">AGENTS.md</span> / <span class="mono">CLAUDE.md</span> under this root.
		</p>

		{#if error}
			<p class="bad" role="alert" data-testid="project-sync-error">{error}</p>
		{:else if report}
			<p class="summary">
				{wasDryRun ? 'Planned' : 'Done'}: {report.created} created, {report.updated} updated,
				{report.unchanged} unchanged, {report.skipped} skipped
			</p>
			<Table
				id="project-sync-ops"
				columns={syncColumns}
				rows={report.ops}
				rowKey={(op: SyncOp) => op.path}
			>
				{#snippet cell(op: SyncOp, column: TableColumn<SyncOp>)}
					{#if column.key === 'path'}
						{op.path}
					{:else}
						{actionLabel(op)}
					{/if}
				{/snippet}
			</Table>
		{:else}
			<div class="empty">
				<Icon name="info" size={20} color="var(--text-tertiary)" />
				<span class="title">No sync run yet</span>
				<span class="hint2">Check shows what would be written without touching any file.</span>
			</div>
		{/if}
	</div>
</div>

<style>
	.card {
		background: var(--bg-raised);
		border: 1px solid var(--border-default);
		border-radius: 5px;
		overflow: hidden;
		display: flex;
		flex-direction: column;
	}

	.head {
		height: 32px;
		flex: 0 0 32px;
		display: flex;
		align-items: center;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.body {
		padding: 12px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}

	.controls {
		display: flex;
		align-items: flex-end;
		gap: 16px;
	}

	.scope {
		width: 320px;
	}

	.targets {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.label {
		color: var(--text-secondary);
	}

	.checks {
		display: flex;
		align-items: center;
		gap: 14px;
		height: 28px;
	}

	.spacer {
		flex: 1;
	}

	.buttons {
		display: flex;
		gap: 8px;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.mono {
		font-family: var(--font-mono);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}

	.summary {
		margin: 0;
		color: var(--text-secondary);
		font-size: 13px;
	}

	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 4px;
		padding: 20px 0;
		border-top: 1px solid var(--border-subtle);
	}

	.title {
		font-weight: 600;
	}

	.hint2 {
		color: var(--text-secondary);
	}
</style>
