<script lang="ts">
	// Projects: the connected list plus Connect. Inside Tauri that opens the native
	// folder picker; in a browser there is no picker, so the path is typed.
	import { onMount, untrack } from 'svelte';
	import { page } from '$app/state';
	import { errorMessage } from '$lib/errors';
	import { plural, relativeAge } from '$lib/format';
	import { setStatusItems } from '$lib/shell';
	import {
		connectProject,
		inTauri,
		loadProjects,
		pickProjectRoot,
		projects
	} from '$lib/stores/projects.svelte';
	import type { Project } from '$lib/types';
	import Button from '$lib/ui/Button.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Input from '$lib/ui/Input.svelte';
	import Table from '$lib/ui/Table.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	const columns = [
		{ key: 'name', label: 'Name', width: '200px' },
		{ key: 'root', label: 'Root' },
		{ key: 'remote', label: 'Remote' },
		{ key: 'seen', label: 'Last seen', width: '110px', align: 'right' as const }
	];

	const native = inTauri();
	let root = $state('');

	async function connect() {
		let path = root.trim();
		if (native) {
			const picked = await pickProjectRoot();
			if (!picked) return;
			path = picked;
		}
		if (!path) {
			push('error', 'Enter the project directory first');
			return;
		}
		try {
			const project = await connectProject(path);
			root = '';
			push('success', `Connected ${project.name}`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	onMount(() => {
		void loadProjects();
	});

	// `?connect=1` (the palette's `⌥↵` on Connect a folder…) opens the connect flow: the
	// native picker in Tauri, or the typed-path field in a browser. It reads the URL rather
	// than running once, so a second palette action while this page is open works too.
	$effect(() => {
		const wanted = page.url.searchParams.get('connect') === '1';
		if (!wanted) return;
		untrack(() => {
			if (native) void connect();
			else document.querySelector<HTMLInputElement>('[data-testid="projects-root"]')?.focus();
		});
	});

	$effect(() => {
		setStatusItems({ right: [{ text: plural(projects.items.length, 'project') }] });
	});
</script>

<div class="head">
	<h1>Projects</h1>
	<div class="connect">
		{#if !native}
			<Input
				bind:value={root}
				data-testid="projects-root"
				placeholder="/path/to/project"
				aria-label="Project directory"
				onkeydown={(e) => {
					if (e.key === 'Enter') void connect();
				}}
			/>
		{/if}
		<Button
			variant="primary"
			data-testid="projects-connect"
			disabled={projects.connecting}
			onclick={connect}
		>
			{projects.connecting ? 'Connecting…' : 'Connect'}
		</Button>
	</div>
</div>

{#if projects.error}
	<ErrorState message={projects.error} logPath={projects.errorLogPath ?? undefined}>
		<Button variant="primary" onclick={() => loadProjects()}>Retry</Button>
	</ErrorState>
{:else}
	<Table
		{columns}
		rows={projects.items}
		data-testid="projects-table"
		rowKey={(p: Project) => p.id}
	>
		{#snippet cell(project: Project, key: string)}
			{#if key === 'name'}
				<a href="/projects/{project.id}">{project.name}</a>
			{:else if key === 'root'}
				<code class="path">{project.root_path}</code>
			{:else if key === 'remote'}
				<span class="muted">{project.git_remote ?? '-'}</span>
			{:else}
				<span class="muted">{relativeAge(project.last_seen_at)}</span>
			{/if}
		{/snippet}
		{#snippet empty()}
			{#if projects.loading}
				<EmptyState title="Loading…" />
			{:else}
				<EmptyState
					title="No projects connected"
					hint={native
						? 'Connect a repository to give its agents project-scoped memory.'
						: 'Type a project directory above and press Connect.'}
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
		height: 28px;
		flex: 0 0 28px;
		margin-bottom: var(--space-4);
	}

	.head h1 {
		margin: 0;
		font-size: 15px;
		font-weight: 600;
	}

	.connect {
		display: flex;
		gap: var(--space-2);
		min-width: 320px;
	}

	.path {
		overflow-wrap: anywhere;
	}

	.muted {
		color: var(--text-secondary);
	}
</style>
