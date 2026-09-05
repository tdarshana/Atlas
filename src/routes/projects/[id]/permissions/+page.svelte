<script lang="ts">
	// The project Permissions tab: this project's agent access rules, which used to be a
	// card on Project settings. Each rule says whether the project sets it or takes the
	// global default, read from `GET /api/v1/projects/{id}/access`.
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { Button } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { setStatusItems } from '$lib/shell';
	import { openProject, project, setHeaderActions } from '$lib/stores/project.svelte';
	import AgentAccessForm from '$lib/components/project/AgentAccessForm.svelte';
	import { accessForm, toAccess, type AccessForm } from '$lib/components/project/access';
	import { knownActors } from '$lib/components/project/settings/settings';
	import type { ProjectAccessReport } from '$lib/types';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	/** Actor labels seen in this project's log. A vocabulary, never a rebuild trigger. */
	let sources = $state<string[]>([]);
	let report = $state<ProjectAccessReport | null>(null);
	let form = $state<AccessForm | null>(null);
	/** The form as it was loaded. Dirty is a comparison against this, not a flag. */
	let loaded = $state<AccessForm | null>(null);
	/** The project the form was built for, so a re-read of the same project leaves it alone. */
	let builtFor = $state('');
	let loading = $state(false);
	let error = $state<string | null>(null);
	let saving = $state(false);

	const id = $derived(project.current?.id ?? '');
	const name = $derived(project.current?.name ?? 'Project');
	const dirty = $derived(!!form && JSON.stringify(form) !== JSON.stringify(loaded));

	function build(fetched: ProjectAccessReport, seen: string[]): void {
		const actors = knownActors(seen, fetched.effective);
		form = accessForm(fetched.access, fetched.effective, actors);
		loaded = accessForm(fetched.access, fetched.effective, actors);
		report = fetched;
	}

	async function load(): Promise<void> {
		if (!id) return;
		loading = true;
		try {
			const fetched = await api().projectAccess(id);
			build(fetched, untrack(() => sources));
			builtFor = id;
			error = null;
		} catch (e) {
			error = errorMessage(e);
		} finally {
			loading = false;
		}
	}

	// One read per project. A later visit to the same project keeps whatever was typed;
	// Save reloads deliberately.
	$effect(() => {
		if (!id || id === builtFor) return;
		void untrack(() => load());
	});

	// The tick list is the project's own actors, so the log is read for the labels that have
	// written to it. One page is plenty: this is a vocabulary, not a history.
	$effect(() => {
		const pid = id;
		if (!pid) return;
		let live = true;
		void api()
			.projectLog(pid, { limit: 200 })
			.then((rows) => {
				if (!live) return;
				const seen = [...new Set(rows.map((r) => r.source).filter(Boolean))];
				sources = seen;
				untrack(() => addLabels(seen));
			})
			.catch(() => {
				// A log that will not load costs the extra labels, not the form.
			});
		return () => {
			live = false;
		};
	});

	/**
	 * Adds labels the form does not offer yet, to the form and to the loaded snapshot
	 * alike. A label arriving late must not make the form look edited, and must not undo an
	 * edit already made, so this extends rather than replaces.
	 */
	function addLabels(labels: string[]): void {
		const f = form;
		const l = loaded;
		const r = report;
		if (!f || !l || !r) return;
		const added = knownActors(labels, r.effective).filter((a) => !f.actors.includes(a));
		if (added.length === 0) return;
		const actors = [...f.actors, ...added].sort((a, b) => a.localeCompare(b));
		const fresh = accessForm(r.access, r.effective, added);
		for (const target of [f, l]) {
			// A copy each: the form and its loaded snapshot are meant to be independent, and
			// one array shared between them is the one place they would not be.
			target.actors = [...actors];
			target.memoryWriters = { ...target.memoryWriters, ...fresh.memoryWriters };
			target.taskMovers = { ...target.taskMovers, ...fresh.taskMovers };
		}
	}

	async function save(): Promise<void> {
		const f = form;
		if (!f || !id || saving) return;
		saving = true;
		try {
			await api().setAgentAccess(id, toAccess(f));
			// The project row carries `agent_access`, and the layout header and the MCP tab
			// both read it, so the stored project is re-read rather than patched here.
			await openProject(id, true);
			await load();
			push('success', 'Agent access saved');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			saving = false;
		}
	}

	$effect(() => {
		setStatusItems({ right: [{ text: `${name} · permissions` }] });
	});

	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});
</script>

{#snippet headerActions()}
	<Button
		variant="primary"
		size="sm"
		data-testid="project-permissions-save"
		disabled={!dirty || saving}
		onclick={save}
	>
		{saving ? 'Saving…' : 'Save'}
	</Button>
	<Button
		variant="ghost"
		size="sm"
		data-testid="project-permissions-open-global"
		onclick={() => void goto('/permissions')}
	>
		Open global permissions
	</Button>
{/snippet}

<div class="stack">
	{#if error}
		<ErrorState message={error}>
			<Button size="sm" data-testid="project-permissions-retry" onclick={() => void load()}>
				Retry
			</Button>
		</ErrorState>
	{:else if form}
		<AgentAccessForm bind:form {report} />
	{:else if loading}
		<span class="hint">Reading the project's rules…</span>
	{/if}
</div>

<style>
	.stack {
		display: flex;
		flex-direction: column;
		gap: 12px;
		flex: 1;
		min-height: 0;
		overflow: auto;
	}

	.hint {
		color: var(--text-tertiary);
	}
</style>
