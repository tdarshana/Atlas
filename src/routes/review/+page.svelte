<script lang="ts">
	// Review (frame 09): the pending memories an extractor or a workflow proposed. Accept
	// or reject one row at a time, or accept every row at or above the auto-accept
	// threshold from Settings.
	import { onMount } from 'svelte';
	import { Badge, Button, Table, type TableColumn } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { NOTHING } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import {
		acceptAllAboveThreshold,
		decide,
		loadReview,
		qualifying,
		review,
		followReviewChanges
	} from '$lib/stores/review.svelte';
	import { loadSettings, minConfidence, settingBool, settingString } from '$lib/stores/settings.svelte';
	import type { Memory, MemoryKind } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	const columns: TableColumn<Memory>[] = [
		{ key: 'text', label: 'Text', mono: true },
		{ key: 'kind', label: 'Kind', width: '100px', sortable: true },
		{
			key: 'confidence',
			label: 'Confidence',
			width: '110px',
			align: 'right',
			mono: true,
			sortable: true
		},
		{ key: 'source', label: 'Source', width: '140px', mono: true },
		{ key: 'project', label: 'Project', width: '140px' },
		{ key: 'actions', label: '', width: '150px', align: 'right' }
	];

	const threshold = $derived(minConfidence());
	const qualified = $derived(qualifying());
	const extractionEnabled = $derived(settingBool('extraction.enabled'));
	const model = $derived(settingString('extraction.model'));
	const baseUrl = $derived(settingString('extraction.base_url'));
	let accepting = $state(false);
	let confirming = $state(false);

	/** The question the confirm asks, naming both numbers the user is deciding on. */
	const confirmText = $derived(
		`Accept ${qualified.length} ${qualified.length === 1 ? 'memory' : 'memories'} above ${threshold.toFixed(2)}?`
	);

	/** Insight reads as information; a todo is pending work. The rest carry no tone. */
	function kindTone(kind: MemoryKind): 'neutral' | 'info' | 'warning' {
		if (kind === 'insight') return 'info';
		if (kind === 'todo') return 'warning';
		return 'neutral';
	}

	function projectName(m: Memory): string {
		if (!m.project_id) return 'Global';
		return projects.items.find((p) => p.id === m.project_id)?.name ?? m.project_id;
	}

	function source(m: Memory): string {
		return [m.source_agent, m.source_tool].filter(Boolean).join(' · ') || NOTHING;
	}

	async function accept(m: Memory): Promise<void> {
		try {
			await decide(m.id, 'active');
			push('success', 'Memory accepted');
		} catch (e) {
			// The daemon's `error` string is the whole explanation; show it verbatim.
			push('error', errorMessage(e));
		}
	}

	async function reject(m: Memory): Promise<void> {
		try {
			await decide(m.id, 'rejected');
			push('success', 'Memory rejected');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	// Accepting in bulk moves every qualifying row into recall at once and there is no
	// undo, so it is asked for by name and by count first.
	async function acceptAll(): Promise<void> {
		confirming = false;
		accepting = true;
		try {
			const { accepted, failed, failure } = await acceptAllAboveThreshold();
			if (failure) push('error', `${failed} failed: ${failure}`);
			if (accepted > 0) {
				push('success', `Accepted ${accepted} ${accepted === 1 ? 'memory' : 'memories'}`);
			}
		} finally {
			accepting = false;
		}
	}

	onMount(() => {
		void loadProjects();
		void loadSettings();
		void loadReview();
		return followReviewChanges();
	});

	$effect(() => {
		setStatusItems({ right: [{ text: `${review.items.length} pending` }] });
	});
</script>

<div class="title-row">
	<span class="title">Review</span>
	<span class="spacer"></span>
	<Button
		variant="primary"
		data-testid="review-accept-all"
		disabled={accepting || qualified.length === 0}
		title="Accepts every pending memory with confidence at or above {threshold.toFixed(2)}"
		onclick={() => (confirming = true)}
	>
		Accept all above threshold ({qualified.length})
	</Button>
</div>

<Dialog open={confirming} title="Accept all above threshold?" onclose={() => (confirming = false)}>
	<p class="prose" data-testid="review-accept-all-confirm-text">{confirmText}</p>
	{#snippet footer()}
		<Button onclick={() => (confirming = false)}>Cancel</Button>
		<Button
			variant="primary"
			data-testid="review-accept-all-confirm"
			disabled={accepting}
			onclick={acceptAll}
		>
			{accepting ? 'Accepting…' : 'Accept'}
		</Button>
	{/snippet}
</Dialog>

<div class="lines">
	<span data-testid="review-extraction-status">
		{#if extractionEnabled && baseUrl.trim() === ''}
			Extraction is on, but no base URL is set, so nothing runs.
			<a href="/settings#extraction">Set one in Settings</a>.
		{:else if extractionEnabled}
			Extraction is on · <span class="mono">{model || 'model not set'}</span>
		{:else}
			Extraction is off. <a href="/settings#extraction">Enable it in Settings</a> to propose
			memories from transcripts.
		{/if}
	</span>
	<span class="mono threshold" data-testid="review-threshold">
		Threshold extraction.auto_accept_min_confidence = {threshold.toFixed(2)}
	</span>
</div>

{#if review.error}
	<ErrorState message={review.error} logPath={review.errorLogPath ?? undefined}>
		<Button variant="primary" onclick={() => loadReview()}>Retry</Button>
	</ErrorState>
{:else}
	<div class="list" data-testid="review-table">
		<Table
			id="review"
			{columns}
			rows={review.items}
			rowKey={(m: Memory) => m.id}
			defaultSort={{ key: 'confidence', dir: 'desc' }}
		>
			{#snippet cell(m: Memory, column: TableColumn<Memory>)}
				{#if column.key === 'text'}
					{m.text}
				{:else if column.key === 'kind'}
					<Badge tone={kindTone(m.kind)}>{m.kind}</Badge>
				{:else if column.key === 'confidence'}
					<span class:above={m.confidence >= threshold}>{m.confidence.toFixed(2)}</span>
				{:else if column.key === 'source'}
					<span class="muted">{source(m)}</span>
				{:else if column.key === 'project'}
					<span class="muted">{projectName(m)}</span>
				{:else}
					<span class="actions">
						<Button
							variant="ghost"
							size="sm"
							data-testid="review-accept"
							disabled={review.busy.includes(m.id)}
							onclick={() => accept(m)}
						>
							Accept
						</Button>
						<Button
							variant="ghost"
							size="sm"
							data-testid="review-reject"
							disabled={review.busy.includes(m.id)}
							onclick={() => reject(m)}
						>
							Reject
						</Button>
					</span>
				{/if}
			{/snippet}
			{#snippet empty()}
				<div class="empty">
					{#if review.loading}
						<span class="empty-title">Loading…</span>
					{:else}
						<span class="empty-title">Nothing to review</span>
						<span class="empty-hint">Memories an extractor proposes land here as pending.</span>
					{/if}
				</div>
			{/snippet}
		</Table>
	</div>
{/if}

<style>
	.title-row {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 28px;
		flex: 0 0 28px;
	}

	.title {
		font-size: 15px;
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.lines {
		display: flex;
		flex-direction: column;
		gap: 4px;
		flex: 0 0 auto;
		color: var(--text-secondary);
	}

	.prose {
		margin: 0;
		max-width: 80ch;
	}

	.threshold {
		color: var(--text-tertiary);
	}

	.mono {
		font-family: var(--font-mono);
	}

	.list {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}

	.above {
		color: var(--accent);
	}

	.muted {
		color: var(--text-secondary);
	}

	.actions {
		display: inline-flex;
		gap: 4px;
		justify-content: flex-end;
	}

	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 6px;
		padding: 28px 0;
	}

	.empty-title {
		font-weight: 600;
	}

	.empty-hint {
		color: var(--text-secondary);
	}
</style>
