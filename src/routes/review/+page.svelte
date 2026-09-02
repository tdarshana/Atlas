<script lang="ts">
	// Review: the pending memories an extractor proposed. Accept or reject one row at
	// a time, or accept every row at or above the auto-accept threshold from settings.
	import { onMount } from 'svelte';
	import {
		acceptAllAboveThreshold,
		decide,
		loadReview,
		qualifying,
		review
	} from '$lib/stores/review.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import { loadSettings, minConfidence, settingBool } from '$lib/stores/settings.svelte';
	import { errorMessage } from '$lib/errors';
	import type { Memory } from '$lib/types';
	import Badge from '$lib/ui/Badge.svelte';
	import Button from '$lib/ui/Button.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Table from '$lib/ui/Table.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	const columns = [
		{ key: 'text', label: 'Text' },
		{ key: 'kind', label: 'Kind', width: '110px' },
		{ key: 'confidence', label: 'Confidence', width: '100px', align: 'right' as const },
		{ key: 'source', label: 'Source', width: '150px' },
		{ key: 'project', label: 'Project', width: '140px' },
		{ key: 'actions', label: '', width: '170px', align: 'right' as const }
	];

	const threshold = $derived(minConfidence());
	const qualified = $derived(qualifying());
	const extractionEnabled = $derived(settingBool('extraction.enabled'));
	let accepting = $state(false);

	function projectName(m: Memory): string {
		if (!m.project_id) return 'Global';
		return projects.items.find((p) => p.id === m.project_id)?.name ?? m.project_id;
	}

	function source(m: Memory): string {
		return [m.source_agent, m.source_tool].filter(Boolean).join(' · ') || 'not set';
	}

	async function accept(m: Memory) {
		try {
			await decide(m.id, 'active');
			push('success', 'Memory accepted');
		} catch (e) {
			// The daemon's `error` string is the whole explanation; show it verbatim.
			push('error', errorMessage(e));
		}
	}

	async function reject(m: Memory) {
		try {
			await decide(m.id, 'rejected');
			push('success', 'Memory rejected');
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	async function acceptAll() {
		accepting = true;
		try {
			const { accepted, failed, failure } = await acceptAllAboveThreshold();
			if (failure) push('error', `${failed} failed: ${failure}`);
			if (accepted > 0) push('success', `Accepted ${accepted} ${accepted === 1 ? 'memory' : 'memories'}`);
		} finally {
			accepting = false;
		}
	}

	onMount(() => {
		void loadProjects();
		void loadSettings();
		void loadReview();
	});
</script>

<header class="head">
	<h1>Review</h1>
	<Button
		variant="primary"
		data-testid="review-accept-all"
		disabled={accepting || qualified.length === 0}
		title="Accepts every pending memory with confidence at or above {threshold.toFixed(2)}"
		onclick={acceptAll}
	>
		Accept all above threshold ({qualified.length})
	</Button>
</header>

<p class="muted" data-testid="review-extraction-status">
	Extraction: {extractionEnabled ? 'enabled' : 'disabled'}.
	{#if !extractionEnabled}
		<a href="/settings">Enable it in Settings</a> to propose memories from transcripts.
	{/if}
</p>

<p class="muted">
	Pending memories wait here until someone accepts them. The threshold is
	<code>extraction.auto_accept_min_confidence</code> = {threshold.toFixed(2)}.
</p>

{#if review.error}
	<ErrorState message={review.error} logPath={review.errorLogPath ?? undefined}>
		<Button variant="primary" onclick={() => loadReview()}>Retry</Button>
	</ErrorState>
{:else}
	<Table
		{columns}
		rows={review.items}
		data-testid="review-table"
		rowKey={(m: Memory) => m.id}
	>
		{#snippet cell(m: Memory, key: string)}
			{#if key === 'text'}
				<span class="text">{m.text}</span>
			{:else if key === 'kind'}
				<Badge>{m.kind}</Badge>
			{:else if key === 'confidence'}
				<span class:strong={m.confidence >= threshold}>{m.confidence.toFixed(2)}</span>
			{:else if key === 'source'}
				<span class="muted">{source(m)}</span>
			{:else if key === 'project'}
				<span class="muted">{projectName(m)}</span>
			{:else}
				<span class="actions">
					<Button
						size="sm"
						variant="primary"
						data-testid="review-accept"
						disabled={review.busy.includes(m.id)}
						onclick={() => accept(m)}
					>
						Accept
					</Button>
					<Button
						size="sm"
						variant="danger"
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
			{#if review.loading}
				<EmptyState title="Loading…" />
			{:else}
				<EmptyState
					title="Nothing to review"
					hint="Memories an extractor proposes land here as pending."
				/>
			{/if}
		{/snippet}
	</Table>
{/if}

<style>
	.head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-4);
	}

	.head h1 {
		margin: 0;
	}

	.muted {
		color: var(--muted);
		font-size: 13px;
	}

	.text {
		display: block;
		max-width: 60ch;
		overflow-wrap: anywhere;
	}

	.strong {
		color: var(--accent);
	}

	.actions {
		display: inline-flex;
		gap: var(--space-2);
		justify-content: flex-end;
	}
</style>
