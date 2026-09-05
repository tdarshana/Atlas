<script lang="ts">
	// The Project settings tab, frame 02.7: the project card, extraction, the danger zone
	// and a Save/Reload row. The page owns the form; the cards own their layout. Save sends
	// both writes in the order the daemon reads them, project first, so a rename that fails
	// stops there. Agent access moved to the Permissions tab, which is where the global
	// defaults a rule can inherit are visible.
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { Button } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { setStatusItems } from '$lib/shell';
	import { mirrorKey, vaultStatus } from '$lib/shell/vault';
	import { openProject, project, remove, setHeaderActions } from '$lib/stores/project.svelte';
	import RemoveProjectDialog from '$lib/components/project/RemoveProjectDialog.svelte';
	import DangerCard from '$lib/components/project/settings/DangerCard.svelte';
	import ExtractionCard from '$lib/components/project/settings/ExtractionCard.svelte';
	import ProjectCard from '$lib/components/project/settings/ProjectCard.svelte';
	import {
		boardKeyBase,
		extractionForm,
		type ExtractionForm,
		KEY_PREFIX_RE,
		keyPrefixConfirm,
		toProjectExtraction
	} from '$lib/components/project/settings/settings';
	import type { Project, ProjectPatch, VaultStatus } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import { push } from '$lib/platform/toasts.svelte';

	interface Form {
		name: string;
		boardKey: string;
		remote: string;
		extraction: ExtractionForm;
	}

	let form = $state<Form | null>(null);
	/** The form as it was loaded. Dirty is a comparison against this, not a flag. */
	let loaded = $state<Form | null>(null);
	/** The project the form was built for, so a re-read of the same project leaves it alone. */
	let builtFor = $state('');
	let saving = $state(false);
	let savedAt = $state<string | null>(null);
	let testing = $state(false);
	let testResult = $state<{ ok: boolean; text: string } | null>(null);
	let renaming = $state<{ from: string; to: string; text: string } | null>(null);
	let confirmingRemove = $state(false);
	/** Whether the desktop vault is open, so a freshly typed key gets mirrored on save. */
	let vault = $state<VaultStatus>('missing');

	const id = $derived(page.params.id ?? '');
	const current = $derived(project.current);
	const name = $derived(current?.name ?? 'Project');
	const dirty = $derived(!!form && JSON.stringify(form) !== JSON.stringify(loaded));

	function formFrom(p: Project): Form {
		return {
			name: p.name,
			boardKey: p.board_key ?? '',
			remote: p.git_remote ?? '',
			extraction: extractionForm(p.extraction)
		};
	}

	/** Builds the form from the project as it stands and calls that the loaded state. */
	function build(p: Project): void {
		form = formFrom(p);
		loaded = formFrom(p);
		builtFor = p.id;
		testResult = null;
	}

	/** Re-reads the project from the daemon, then rebuilds the form from what came back. */
	async function reload(): Promise<void> {
		if (!id) return;
		try {
			await openProject(id, true);
		} catch (e) {
			push('error', errorMessage(e));
		}
		const p = project.current;
		if (p) build(p);
	}

	// Build the form once per project. A later read of the same project, from the layout or
	// from Retry, leaves whatever the user has typed alone; `reload` and a save rebuild it
	// deliberately.
	$effect(() => {
		const p = current;
		if (!p || p.id === builtFor) return;
		untrack(() => build(p));
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

	// Loaded once: unlike the project itself, the vault is not per-project state, and the
	// Security card (Settings) is where it is opened or closed.
	$effect(() => {
		void vaultStatus().then((s) => (vault = s));
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
		// before it is sent rather than reported afterwards. `taskCounts` covers every stage
		// of the effective list, done stages included; a task parked on a stage name the
		// list no longer holds is renamed but not counted here.
		//
		// A project connected before migration 3 has no stored key, and the daemon renames
		// from the one it derives from the name, so the confirm has to name that rather than
		// wave the rename through unannounced.
		const from = p.board_key ?? boardKeyBase(p.name);
		if (key && key !== from) {
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

	/**
	 * The two writes, each reported by name. They run in sequence because the daemon reads
	 * them that way, but a failure in one no longer hides the one after it, and anything
	 * that did land is read back so the next Save does not send it again.
	 */
	async function commit() {
		const f = form;
		const p = current;
		if (!f || !p) return;
		renaming = null;
		saving = true;
		let sent = 0;
		const landed = { project: false, extraction: false };
		const failed: string[] = [];

		const write = async (what: string, part: keyof typeof landed, fn: () => Promise<unknown>) => {
			sent++;
			try {
				await fn();
				landed[part] = true;
			} catch (e) {
				failed.push(`${what}: ${errorMessage(e)}`);
			}
		};

		const patch = patchOf(f, p);
		if (Object.keys(patch).length > 0) {
			await write('Project', 'project', () => api().patchProject(p.id, patch));
		}

		if (JSON.stringify(f.extraction) !== JSON.stringify(extractionForm(p.extraction))) {
			await write('Extraction', 'extraction', () =>
				api().setProjectExtraction(p.id, toProjectExtraction(f.extraction))
			);
			// A freshly typed key (not a masked round trip) mirrors into the vault once the
			// write itself has landed; a mirror failure is reported but does not undo it.
			if (landed.extraction && f.extraction.apiKey && vault === 'unlocked') {
				try {
					await mirrorKey(`project/${p.id}`, f.extraction.apiKey);
				} catch (e) {
					push('error', `Vault: ${errorMessage(e)}`);
				}
			}
		}

		if (Object.values(landed).some(Boolean)) {
			await reloadKeeping(landed);
			savedAt = clockNow();
		}
		saving = false;
		// A Save with nothing to send made no request; saying it saved would be a claim
		// about a write that never happened.
		if (failed.length > 0) push('error', failed.join('; '));
		else if (sent > 0) push('success', 'Settings saved');
	}

	/**
	 * Re-reads the project after a save and takes the stored values for the cards that
	 * landed, leaving the ones that failed exactly as the user typed them. `loaded` always
	 * becomes the stored truth, so the failed card alone stays dirty and Save can retry it.
	 */
	async function reloadKeeping(landed: { project: boolean; extraction: boolean }): Promise<void> {
		const before = form;
		try {
			await openProject(id, true);
		} catch (e) {
			push('error', errorMessage(e));
		}
		const p = project.current;
		if (!p || !before) return;
		const stored = formFrom(p);
		form = {
			...stored,
			...(landed.project ? {} : { name: before.name, boardKey: before.boardKey, remote: before.remote }),
			...(landed.extraction ? {} : { extraction: before.extraction })
		};
		loaded = formFrom(p);
		builtFor = p.id;
		testResult = null;
	}

	/**
	 * The daemon tests the *stored* override, so an edited endpoint has to land before the
	 * test means anything. Saves the extraction card first when it differs, exactly as the
	 * global settings screen does, and gives up if that write fails.
	 */
	async function onTest() {
		if (!id || testing || saving) return;
		const f = form;
		const p = current;
		if (f && p && JSON.stringify(f.extraction) !== JSON.stringify(extractionForm(p.extraction))) {
			saving = true;
			try {
				await api().setProjectExtraction(p.id, toProjectExtraction(f.extraction));
			} catch (e) {
				saving = false;
				push('error', `Extraction: ${errorMessage(e)}`);
				return;
			}
			saving = false;
			await reloadKeeping({ project: false, extraction: true });
		}
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

<RemoveProjectDialog
	open={confirmingRemove}
	{name}
	testid="settings-remove-confirm"
	onclose={() => (confirmingRemove = false)}
	onconfirm={confirmRemove}
/>

{#if form && current}
	<div class="stack">
		<ProjectCard
			bind:name={form.name}
			bind:boardKey={form.boardKey}
			bind:remote={form.remote}
			root={current.root_path}
		/>
		<section class="card" data-testid="settings-access-moved">
			<header><span class="card-title">Agent access</span></header>
			<div class="card-body">
				<span class="hint">
					Agent access rules moved to the Permissions tab, which also shows the global
					defaults a rule inherits.
				</span>
				<a href="/projects/{current.id}/permissions" data-testid="settings-access-link">
					Open the Permissions tab
				</a>
			</div>
		</section>
		<ExtractionCard
			bind:form={form.extraction}
			{testing}
			result={testResult}
			vaultStatus={vault}
			ontest={onTest}
		/>
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
			<Button data-testid="settings-reload" disabled={saving} onclick={() => void reload()}>
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

	/* The pointer card, styled like the ones the settings components draw. */
	.card {
		display: flex;
		flex-direction: column;
		flex: 0 0 auto;
	}

	.card header {
		display: flex;
		align-items: center;
		height: 32px;
		flex: 0 0 32px;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.card-title {
		font-weight: 600;
	}

	.card-body {
		display: flex;
		flex-direction: column;
		gap: 8px;
		padding: 12px;
	}

	.hint {
		color: var(--text-secondary);
		max-width: 80ch;
	}
</style>
