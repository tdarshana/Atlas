<script lang="ts">
	// The project hub: a 28px header naming the project and carrying the open tab's
	// actions, the 34px tab strip, then the tab itself. Global is the every-project scope,
	// so it has no root path and only the two tabs that mean anything without a project.
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import type { Snippet } from 'svelte';
	import { daemon } from '$lib/daemon.svelte';
	import { shell, TabStrip } from '$lib/shell';
	import { GLOBAL_ID, openProject, project, tabForPath, tabsFor } from '$lib/stores/project.svelte';
	import Button from '$lib/ds/Button.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';

	let { children, data }: { children: Snippet; data: { id: string; isGlobal: boolean } } =
		$props();

	const id = $derived(data.id);
	const isGlobal = $derived(data.isGlobal);
	const tabs = $derived(tabsFor(id));
	const active = $derived(tabForPath(page.url.pathname));
	const name = $derived(isGlobal ? 'Global' : (project.current?.name ?? 'Project'));
	const root = $derived(isGlobal ? '' : (project.current?.root_path ?? ''));

	// The daemon owns the port, so the project can only be fetched once it has answered.
	$effect(() => {
		if (daemon.ready && id) void openProject(id);
	});

	// Global has no profile, so its own route is the board.
	$effect(() => {
		if (isGlobal && active === 'profile') void goto(`/projects/${GLOBAL_ID}/board`, { replaceState: true });
	});

	// The side panel otherwise reads "Projects" here; naming the open project is better.
	$effect(() => {
		shell.sidePanelTitle = isGlobal ? 'Projects' : (project.current?.name ?? 'Projects');
	});
</script>

<div class="head">
	<span class="mono name">{name}</span>
	{#if root}<span class="mono root">{root}</span>{/if}
	<span class="spacer"></span>
</div>

<!-- A project that would not load has no tabs worth offering: every one of them reads the
     project this route names, so the whole hub is the failure, not the open tab. -->
{#if project.error}
	<ErrorState message={project.error} logPath={project.errorLogPath ?? undefined}>
		<Button
			variant="primary"
			data-testid="project-retry"
			onclick={() => void openProject(id, true)}
		>
			Retry
		</Button>
	</ErrorState>
{:else}
	<TabStrip items={tabs} {active} />

	<!-- Each tab's own controls sit inside the tab, under the strip, not in the hub
	     header: the header names the project, the toolbar belongs to what is open. -->
	{#if project.actions}
		<div class="tab-toolbar" data-testid="tab-toolbar">
			<span class="spacer"></span>
			<div class="actions">{@render project.actions()}</div>
		</div>
	{/if}

	{@render children()}
{/if}

<style>
	.head {
		display: flex;
		align-items: center;
		gap: 8px;
		height: 28px;
		flex: 0 0 28px;
	}

	.name {
		font-size: 15px;
		font-weight: 600;
	}

	.root {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12px;
		color: var(--text-secondary);
	}

	.spacer {
		flex: 1;
	}

	.tab-toolbar {
		display: flex;
		align-items: center;
		min-height: 28px;
		flex: 0 0 auto;
	}

	.actions {
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}
</style>
