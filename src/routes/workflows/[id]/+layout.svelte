<script lang="ts">
	// The workflow hub header (frame 08): `Workflows / <name>`, the last-run badge, then
	// `Add action` and `Run`, above the Editor / Run history / Settings tab strip.
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';
	import { daemon } from '$lib/daemon.svelte';
	import { Badge, Button } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { shell, TabStrip, type Tab } from '$lib/shell';
	import {
		ACTION_PRESETS,
		addAction,
		openWorkflow,
		run,
		workflow
	} from '$lib/stores/workflows.svelte';
	import type { RunStatus } from '$lib/types';
	import { push } from '$lib/platform/toasts.svelte';

	let { children, data }: { children: Snippet; data: { id: string } } = $props();

	const id = $derived(data.id);

	$effect(() => {
		if (daemon.ready && id) void openWorkflow(id);
	});

	$effect(() => {
		shell.sidePanelTitle = 'Workflows';
	});

	const tabs: Tab[] = $derived([
		{ id: 'editor', label: 'Editor', href: `/workflows/${id}` },
		{ id: 'history', label: 'Run history', href: `/workflows/${id}/history` },
		{ id: 'settings', label: 'Settings', href: `/workflows/${id}/settings` }
	]);

	const active = $derived(
		page.url.pathname.endsWith('/settings')
			? 'settings'
			: page.url.pathname.endsWith('/history')
				? 'history'
				: 'editor'
	);

	const LAST_RUN: Record<'never' | RunStatus, { text: string; tone: 'neutral' | 'success' | 'danger' | 'info' }> = {
		never: { text: 'never run', tone: 'neutral' },
		success: { text: 'last run ok', tone: 'success' },
		failed: { text: 'last run failed', tone: 'danger' },
		cancelled: { text: 'last run cancelled', tone: 'neutral' },
		queued: { text: 'last run pending', tone: 'info' },
		running: { text: 'last run pending', tone: 'info' }
	};

	const lastRun = $derived(LAST_RUN[workflow.current?.last_status ?? 'never']);

	async function addBlankAction(): Promise<void> {
		addAction(ACTION_PRESETS[ACTION_PRESETS.length - 1]);
	}

	async function runNow(): Promise<void> {
		try {
			const r = await run();
			push('success', `Run ${r.number} queued`);
			await goto(`/workflows/${id}/history?run=${r.id}`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

<div class="head">
	<span class="title">Workflows</span>
	<span class="sep">/</span>
	<span class="mono title">{workflow.current?.name ?? id}</span>
	<Badge tone={lastRun.tone}>{lastRun.text}</Badge>
	<span class="spacer"></span>
	<Button data-testid="workflow-add-action" onclick={addBlankAction} disabled={!workflow.current}>
		Add action
	</Button>
	<Button
		variant="primary"
		data-testid="workflow-run"
		disabled={!workflow.current}
		onclick={runNow}
	>
		Run
	</Button>
</div>

<TabStrip items={tabs} {active} />

{@render children()}

<style>
	.head {
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

	.sep {
		color: var(--text-tertiary);
	}

	.spacer {
		flex: 1;
	}
</style>
