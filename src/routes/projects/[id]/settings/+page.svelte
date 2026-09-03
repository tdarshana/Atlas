<script lang="ts">
	// The Project settings tab, frame 02.7: four stacked cards and a Save/Reload row. The
	// page owns the form; the cards own their layout. Save sends the three writes in the
	// order the daemon reads them, project first, so a rename that fails stops there.
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { Button } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { setStatusItems } from '$lib/shell';
	import { openProject, project, remove, setHeaderActions } from '$lib/stores/project.svelte';
	import AgentAccessCard from '$lib/components/project/settings/AgentAccessCard.svelte';
	import DangerCard from '$lib/components/project/settings/DangerCard.svelte';
	import ExtractionCard from '$lib/components/project/settings/ExtractionCard.svelte';
	import ProjectCard from '$lib/components/project/settings/ProjectCard.svelte';
	import {
		accessChecked,
		extractionForm,
		type ExtractionForm,
		KEY_PREFIX_RE,
		keyPrefixConfirm,
		knownActors,
		toAgentAccess,
		toProjectExtraction
	} from '$lib/components/project/settings/settings';
	import type { Project, ProjectPatch } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	interface Form {
		name: string;
		boardKey: string;
		remote: string;
		actors: string[];
		checked: Record<string, boolean>;
		requireReview: boolean;
		extraction: ExtractionForm;
	}

	/** Actor labels seen in this project's log, which is where the tick list comes from. */
	let sources = $state<string[]>([]);
	let form = $state<Form | null>(null);
	/** The form as it was loaded. Dirty is a comparison against this, not a flag. */
	let loaded = $state('');
	let saving = $state(false);
	let savedAt = $state<string | null>(null);
	let testing = $state(false);
	let testResult = $state<{ ok: boolean; text: string } | null>(null);
	let renaming = $state<{ from: string; to: string; text: string } | null>(null);
	let confirmingRemove = $state(false);

	const id = $derived(page.params.id ?? '');
	const current = $derived(project.current);
	const name = $derived(current?.name ?? 'Project');
	const dirty = $derived(!!form && JSON.stringify(form) !== loaded);

	function formFrom(p: Project, seen: string[]): Form {
		const actors = knownActors(seen, p.agent_access);
		return {
			name: p.name,
			boardKey: p.board_key ?? '',
			remote: p.git_remote ?? '',
			actors,
			checked: accessChecked(actors, p.agent_access),
			requireReview: p.agent_access.require_review,
			extraction: extractionForm(p.extraction)
		};
	}

	function reset(): void {
		if (!current) return;
		const next = formFrom(current, sources);
		form = next;
		loaded = JSON.stringify(next);
		testResult = null;
	}

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
				sources = [...new Set(rows.map((r) => r.source).filter(Boolean))];
			})
			.catch(() => {
				// A log that will not load costs the extra labels, not the card.
				if (live) sources = [];
			});
		return () => {
			live = false;
		};
	});

	// Rebuild the form whenever the project or the label list lands. Untracked because
	// `reset` reads and writes the form itself, which would otherwise loop.
	$effect(() => {
		const p = current;
		const seen = sources;
		if (!p) return;
		untrack(() => {
			const next = formFrom(p, seen);
			form = next;
			loaded = JSON.stringify(next);
		});
	});

	$effect(() => {
		setStatusItems({
			right: [{ text: savedAt ? `${name} · settings saved ${savedAt}` : name }]
		});
	});

	// The frame gives this tab no header buttons; clearing the previous tab's is the point.
	$effect(() => {
		setHeaderActions(null);
	});

	/** `15:15`, the time the save landed. */
	function clockNow(): string {
		const d = new Date();
		return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
	}

	/** The project fields that changed. `git_remote: null` clears; absent leaves alone. */
	function patchOf(f: Form, p: Project): ProjectPatch {
		const patch: ProjectPatch = {};
		if (f.name.trim() && f.name.trim() !== p.name) patch.name = f.name.trim();
		const key = f.boardKey.trim().toUpperCase();
		if (key && key !== (p.board_key ?? '')) patch.board_key = key;
		const remote = f.remote.trim();
		if (remote !== (p.git_remote ?? '')) patch.git_remote = remote || null;
		return patch;
	}

	async function onSave() {
		const f = form;
		const p = current;
		if (!f || !p || saving) return;

		const key = f.boardKey.trim().toUpperCase();
		if (key && !KEY_PREFIX_RE.test(key)) {
			push('error', 'Key prefix must be 2 to 6 characters, a letter then letters and digits.');
			return;
		}

		// A prefix change rewrites every task key in the project, so it is named and counted
		// before it is sent rather than reported afterwards.
		const from = p.board_key ?? '';
		if (key && from && key !== from) {
			try {
				const counts = await api().taskCounts(p.id);
				const total = counts.reduce((n, c) => n + c.count, 0);
				renaming = { from, to: key, text: keyPrefixConfirm(total, from, key) };
			} catch (e) {
				push('error', errorMessage(e));
			}
			return;
		}
		await commit();
	}

	async function commit() {
		const f = form;
		const p = current;
		if (!f || !p) return;
		renaming = null;
		saving = true;
		try {
			const patch = patchOf(f, p);
			if (Object.keys(patch).length > 0) await api().patchProject(p.id, patch);

			const access = toAgentAccess(f.actors, f.checked, f.requireReview);
			if (JSON.stringify(access) !== JSON.stringify(p.agent_access)) {
				await api().setAgentAccess(p.id, access);
			}

			const before = JSON.stringify(extractionForm(p.extraction));
			if (JSON.stringify(f.extraction) !== before) {
				await api().setProjectExtraction(p.id, toProjectExtraction(f.extraction));
			}

			await openProject(p.id, true);
			savedAt = clockNow();
			reset();
			push('success', 'Settings saved');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			saving = false;
		}
	}

	async function onTest() {
		if (!id || testing) return;
		testing = true;
		testResult = null;
		try {
			const res = await api().testExtraction(id);
			testResult = { ok: res.ok, text: res.ok ? (res.reply ?? 'Connected.') : (res.error ?? 'Failed.') };
		} catch (e) {
			testResult = { ok: false, text: errorMessage(e) };
		} finally {
			testing = false;
		}
	}

	async function confirmRemove() {
		const label = name;
		try {
			await remove();
			confirmingRemove = false;
			// This page's project is gone, so leave before its state is read again.
			await goto('/projects');
			push('success', `Removed ${label}`);
		} catch (e) {
			push('error', errorMessage(e));
		}
	}
</script>

<Dialog open={!!renaming} title="Change the key prefix?" onclose={() => (renaming = null)}>
	<p class="prose">{renaming?.text}</p>
	{#snippet footer()}
		<Button onclick={() => (renaming = null)}>Cancel</Button>
		<Button variant="primary" data-testid="rename-confirm" onclick={() => void commit()}>
			Rename
		</Button>
	{/snippet}
</Dialog>

<Dialog
	open={confirmingRemove}
	title="Remove this project?"
	onclose={() => (confirmingRemove = false)}
>
	<p class="prose">
		Atlas forgets <strong>{name}</strong> and stops offering it as a scope. Its memories are kept,
		its tasks are deleted, and connecting the same root again re-adds it. Nothing on disk is
		touched.
	</p>
	{#snippet footer()}
		<Button onclick={() => (confirmingRemove = false)}>Cancel</Button>
		<Button
			variant="danger"
			data-testid="settings-remove-confirm"
			disabled={project.removing}
			onclick={confirmRemove}
		>
			{project.removing ? 'Removing…' : 'Remove'}
		</Button>
	{/snippet}
</Dialog>

{#if form && current}
	<div class="stack">
		<ProjectCard
			bind:name={form.name}
			bind:boardKey={form.boardKey}
			bind:remote={form.remote}
			root={current.root_path}
		/>
		<AgentAccessCard
			bind:actors={form.actors}
			bind:checked={form.checked}
			bind:requireReview={form.requireReview}
		/>
		<ExtractionCard bind:form={form.extraction} {testing} result={testResult} ontest={onTest} />
		<DangerCard disabled={project.removing} onremove={() => (confirmingRemove = true)} />

		<div class="actions">
			<Button
				variant="primary"
				data-testid="settings-save"
				disabled={!dirty || saving}
				onclick={onSave}
			>
				{saving ? 'Saving…' : 'Save'}
			</Button>
			<Button data-testid="settings-reload" disabled={!dirty || saving} onclick={reset}>
				Reload
			</Button>
		</div>
	</div>
{/if}

<style>
	.stack {
		display: flex;
		flex-direction: column;
		gap: 12px;
		flex: 1;
		min-height: 0;
		overflow: auto;
	}

	.actions {
		display: flex;
		gap: 8px;
	}

	.prose {
		margin: 0;
		max-width: 80ch;
	}
</style>
