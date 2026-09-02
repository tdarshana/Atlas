<script lang="ts">
	// Placeholder dashboard: Task 4 adds the project/agent counts and the recent
	// memories list. Everything here reads the polled status store.
	import { status } from '$lib/stores/status.svelte';
	import Badge from '$lib/ui/Badge.svelte';
	import Card from '$lib/ui/Card.svelte';

	const report = $derived(status.report);
	const embedding = $derived(report?.embedding ?? '');
	const tone: 'success' | 'accent' | 'danger' = $derived(
		embedding.startsWith('ready') ? 'success' : embedding.startsWith('loading') ? 'accent' : 'danger'
	);
</script>

<h1>Dashboard</h1>

<div class="grid">
	<Card title="Active memories">
		<p class="metric">{report?.memories_active ?? '—'}</p>
	</Card>

	<Card title="Embedding">
		{#if embedding}
			<Badge {tone}>{embedding}</Badge>
		{:else}
			<span class="muted">unknown</span>
		{/if}
	</Card>

	<Card title="Daemon">
		<dl>
			<dt>Version</dt>
			<dd>{report?.version ?? '—'}</dd>
			<dt>Port</dt>
			<dd>{report?.port ?? '—'}</dd>
			<dt>Database</dt>
			<dd><code>{report?.db_path || '—'}</code></dd>
		</dl>
	</Card>
</div>

<style>
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
		gap: var(--space-4);
	}

	.metric {
		margin: 0;
		font-size: 28px;
		font-weight: 600;
	}

	.muted {
		color: var(--muted);
	}

	dl {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: var(--space-1) var(--space-3);
		margin: 0;
	}

	dt {
		color: var(--muted);
	}

	dd {
		margin: 0;
		overflow-wrap: anywhere;
	}
</style>
