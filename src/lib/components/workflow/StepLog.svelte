<script lang="ts">
	// One row of the run detail's per-step list (frame 08.1): a status icon, the step's
	// name, its agent, a duration, and an expander revealing the log lines plus a
	// collapsed-by-default output pane. `WorkflowStep` covers actions only: the trigger
	// and the output node make no model call of their own, so there is one row per
	// action and nothing for either fixed endpoint.
	import { Badge, Icon } from '$lib/ds';
	import { duration } from '$lib/format';
	import type { LogLevel, StepStatus, WorkflowStep } from '$lib/types';

	let { step }: { step: WorkflowStep } = $props();

	let expanded = $state(false);
	let showOutput = $state(false);

	const STEP_ICON: Record<StepStatus, { name: string; color: string }> = {
		success: { name: 'circle-check', color: 'var(--success-text)' },
		failed: { name: 'circle-x', color: 'var(--danger-text)' },
		running: { name: 'loader', color: 'var(--accent)' },
		queued: { name: 'circle', color: 'var(--text-tertiary)' },
		skipped: { name: 'circle', color: 'var(--text-tertiary)' },
		cancelled: { name: 'circle-x', color: 'var(--text-tertiary)' }
	};

	const STATUS_TONE: Record<StepStatus, 'success' | 'danger' | 'warning' | 'neutral'> = {
		success: 'success',
		failed: 'danger',
		running: 'warning',
		queued: 'warning',
		skipped: 'neutral',
		cancelled: 'neutral'
	};

	const STATUS_LABEL: Record<StepStatus, string> = {
		success: 'ok',
		failed: 'failed',
		running: 'running',
		queued: 'queued',
		skipped: 'skipped',
		cancelled: 'cancelled'
	};

	const LEVEL_TONE: Record<LogLevel, 'neutral' | 'warning' | 'danger'> = {
		INFO: 'neutral',
		WARN: 'warning',
		ERR: 'danger'
	};

	const icon = $derived(STEP_ICON[step.status]);

	function toggle(): void {
		expanded = !expanded;
	}

	function toggleOutput(): void {
		showOutput = !showOutput;
	}
</script>

<div class="step" data-testid="step-row">
	<button type="button" class="row" onclick={toggle} data-testid="step-toggle">
		<Icon name={expanded ? 'chevron-down' : 'chevron-right'} size={12} color="var(--text-tertiary)" />
		<Icon name={icon.name} size={13} color={icon.color} />
		<span class="name">{step.name}</span>
		<Badge tone="accent" mono>{step.agent}</Badge>
		<span class="spacer"></span>
		<Badge tone={STATUS_TONE[step.status]}>{STATUS_LABEL[step.status]}</Badge>
		<span class="mono time">{duration(step.started_at, step.finished_at)}</span>
	</button>

	{#if expanded}
		<div class="body" data-testid="step-log">
			{#each step.log as line, i (i)}
				<div class="line">
					<span class="mono ts">{line.ts}</span>
					<Badge tone={LEVEL_TONE[line.level]}>{line.level}</Badge>
					<span class="text">{line.text}</span>
				</div>
			{:else}
				<p class="hint">No log lines.</p>
			{/each}

			{#if step.output}
				<button
					type="button"
					class="output-toggle"
					onclick={toggleOutput}
					data-testid="step-output-toggle"
				>
					{showOutput ? 'Hide output' : 'Show output'}
				</button>
				{#if showOutput}
					<pre class="mono output" data-testid="step-output">{step.output}</pre>
				{/if}
			{/if}
		</div>
	{/if}
</div>

<style>
	.step {
		display: flex;
		flex-direction: column;
		border-bottom: 1px solid var(--border-subtle);
	}

	.row {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 28px;
		flex: 0 0 28px;
		padding: 0 12px;
		border: 0;
		background: transparent;
		color: inherit;
		font: inherit;
		text-align: left;
		cursor: pointer;
	}

	.row:hover {
		background: var(--bg-hover);
	}

	.name {
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.spacer {
		flex: 1;
	}

	.time {
		color: var(--text-tertiary);
	}

	.body {
		display: flex;
		flex-direction: column;
		gap: 2px;
		background: var(--bg-base);
		border-top: 1px solid var(--border-subtle);
		padding: 6px 12px 10px 38px;
	}

	.hint {
		margin: 0;
		color: var(--text-secondary);
	}

	.line {
		display: flex;
		align-items: baseline;
		gap: 12px;
		font-size: 12px;
		line-height: 20px;
	}

	.ts {
		color: var(--text-tertiary);
	}

	.text {
		color: var(--text-secondary);
		white-space: pre-wrap;
	}

	.output-toggle {
		align-self: flex-start;
		margin-top: 6px;
		padding: 0;
		border: 0;
		background: transparent;
		color: var(--accent);
		font-size: 12px;
		cursor: pointer;
	}

	.output {
		margin: 6px 0 0;
		padding: 8px;
		background: var(--bg-inset);
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		font-size: 12px;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}
</style>
