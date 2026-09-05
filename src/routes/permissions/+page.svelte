<script lang="ts">
	// The Permissions view: what Atlas is allowed to do, in three sections. The macOS
	// permissions it needs from the OS, with live status; what each installed plugin may
	// do, revocable; and the agent access defaults every project inherits unless it sets
	// its own rule on its Permissions tab.
	import { onMount, untrack } from 'svelte';
	import { page } from '$app/state';
	import { Badge, Button, Checkbox, Input } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { openSettingsPane, permissionRequest } from '$lib/permissions/commands';
	import { checkPermissions, putStatus, systemPermissions } from '$lib/permissions/store.svelte';
	import { grantedCount, toRows, type PermissionId, type PermissionRow } from '$lib/permissions/system';
	import { asksForPermissions, nextGrants, permissionChips } from '$lib/plugins/grants';
	import { loadPlugins, plugins, setPermissions } from '$lib/plugins/host.svelte';
	import type { Permission, PluginInfo } from '$lib/plugins/types';
	import { alwaysAllowed, knownActors } from '$lib/components/project/settings/settings';
	import { inTauri, setStatusItems } from '$lib/shell';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import { loadSettings, saveSettings, settings } from '$lib/stores/settings.svelte';
	import type { AgentAccess } from '$lib/types';
	import { push } from '$lib/ui/toasts.svelte';

	/** How often the system rows are re-read, so a grant made in System Settings shows up
	 * without the user coming back and clicking. */
	const POLL_MS = 5000;

	// -- system ----------------------------------------------------------------------------

	let requesting = $state<PermissionId | null>(null);

	const rows = $derived(toRows(systemPermissions.statuses));
	const counts = $derived(grantedCount(rows));
	const checking = $derived(systemPermissions.checking);

	/** The connected project roots, which the files row probes. */
	const roots = $derived(projects.items.map((p) => p.root_path).filter(Boolean));

	async function check(): Promise<void> {
		await checkPermissions(untrack(() => roots));
		if (systemPermissions.error) push('error', systemPermissions.error);
	}

	async function request(row: PermissionRow): Promise<void> {
		requesting = row.id;
		try {
			const answer = await permissionRequest(row.id);
			if (answer) putStatus(answer);
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			requesting = null;
			// The system prompt may be answered after this resolves, so read the truth back.
			await check();
		}
	}

	// -- plugins ---------------------------------------------------------------------------

	let togglingPlugin = $state<string | null>(null);

	/** The plugins that ask for anything at all. One that asks for nothing has no row to
	 * draw and nothing to revoke. */
	const pluginRows = $derived(plugins.items.filter(asksForPermissions));

	async function togglePermission(
		plugin: PluginInfo,
		permission: Permission,
		on: boolean
	): Promise<void> {
		const next = nextGrants(plugin, permission, on);
		togglingPlugin = plugin.id;
		try {
			await setPermissions(plugin.id, next);
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			togglingPlugin = null;
		}
	}

	// -- agent defaults --------------------------------------------------------------------

	interface DefaultsForm {
		actors: string[];
		anyWriters: boolean;
		memoryWriters: Record<string, boolean>;
		anyMovers: boolean;
		taskMovers: Record<string, boolean>;
		requireReview: boolean;
	}

	let defaults = $state<DefaultsForm | null>(null);
	let loadedDefaults = $state<DefaultsForm | null>(null);
	let savingDefaults = $state(false);
	let draft = $state('');

	const defaultsDirty = $derived(
		!!defaults && JSON.stringify(defaults) !== JSON.stringify(loadedDefaults)
	);
	const trimmed = $derived(draft.trim());
	const canAdd = $derived(!!trimmed && !(defaults?.actors ?? []).includes(trimmed));

	/** A settings value that is null or a list of strings; anything else reads as null. */
	function listOf(value: unknown): string[] | null {
		return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : null;
	}

	function storedDefaults(): AgentAccess {
		return {
			memory_writers: listOf(settings.values['access.memory_writers']),
			task_movers: listOf(settings.values['access.task_movers']),
			require_review: settings.values['access.require_review'] === true
		};
	}

	function defaultsForm(access: AgentAccess): DefaultsForm {
		const actors = knownActors([], access);
		const ticks = (allowed: string[] | null) => {
			const out: Record<string, boolean> = {};
			for (const actor of actors) out[actor] = allowed === null || allowed.includes(actor);
			return out;
		};
		return {
			actors,
			anyWriters: access.memory_writers === null,
			memoryWriters: ticks(access.memory_writers),
			anyMovers: access.task_movers === null,
			taskMovers: ticks(access.task_movers),
			requireReview: access.require_review
		};
	}

	function buildDefaults(): void {
		const access = storedDefaults();
		defaults = defaultsForm(access);
		loadedDefaults = defaultsForm(access);
	}

	function addActor(): void {
		const f = defaults;
		if (!f || !canAdd) return;
		f.actors = [...f.actors, trimmed].sort((a, b) => a.localeCompare(b));
		f.memoryWriters = { ...f.memoryWriters, [trimmed]: true };
		f.taskMovers = { ...f.taskMovers, [trimmed]: true };
		draft = '';
	}

	async function saveDefaults(): Promise<void> {
		const f = defaults;
		if (!f || savingDefaults) return;

		const writers = f.anyWriters ? null : f.actors.filter((a) => f.memoryWriters[a]);
		const movers = f.anyMovers ? null : f.actors.filter((a) => f.taskMovers[a]);
		// The daemon refuses an empty list, and rightly: a rule naming nobody is not the
		// same as "any actor", and quietly sending null instead would say the opposite of
		// what the ticks show.
		if (writers?.length === 0 || movers?.length === 0) {
			push('error', 'A rule that is not "Any actor" needs at least one label ticked.');
			return;
		}

		savingDefaults = true;
		try {
			await saveSettings({
				'access.memory_writers': writers,
				'access.task_movers': movers,
				'access.require_review': f.requireReview
			});
			buildDefaults();
			push('success', 'Agent defaults saved');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			savingDefaults = false;
		}
	}

	// -- boot ------------------------------------------------------------------------------

	onMount(() => {
		void loadPlugins();
		void loadSettings().then(() => buildDefaults());
		// The files row probes the connected roots, so the first read waits for the project
		// list; asking in the same tick would report on no roots at all and then quietly
		// correct itself five seconds later.
		void loadProjects().then(check);

		// While the page is on screen a grant made in System Settings has to show up on its
		// own; a hidden window has nothing to update, so the timer stands down with it.
		const timer = setInterval(() => {
			if (document.visibilityState === 'visible') void check();
		}, POLL_MS);
		return () => clearInterval(timer);
	});

	// A settings load started elsewhere (the Settings screen) lands here too, and the form
	// has to exist before it can be edited.
	$effect(() => {
		if (settings.loaded && !untrack(() => defaults)) untrack(() => buildDefaults());
	});

	$effect(() => {
		setStatusItems({
			right: [{ text: `${counts.granted} of ${counts.total} system permissions granted` }]
		});
	});

	// The side panel links to `/permissions#plugins` and friends; a hash arriving on the URL
	// scrolls to the section it names.
	$effect(() => {
		const hash = page.url.hash.replace('#', '');
		if (!hash) return;
		document.getElementById(hash)?.scrollIntoView({ behavior: 'instant', block: 'start' });
	});
</script>

<div class="stack">
	<section class="card" id="system">
		<header>
			<span class="card-title">System</span>
			<span class="spacer"></span>
			<span class="hint" data-testid="system-counts">
				{counts.granted} of {counts.total} granted
			</span>
			<Button size="sm" data-testid="permissions-recheck" disabled={checking} onclick={check}>
				{checking ? 'Checking…' : 'Check again'}
			</Button>
		</header>
		<div class="body">
			{#if !inTauri()}
				<span class="hint">
					System permissions belong to the desktop app; a browser tab has none to read.
				</span>
			{/if}
			{#each rows as row (row.id)}
				<div class="row" data-testid="permission-{row.id}">
					<div class="what">
						<span class="name">{row.name}</span>
						<span class="hint">{row.why}</span>
						{#if row.note}<span class="hint">{row.note}</span>{/if}
						{#if row.detail}
							<span class="hint mono" data-testid="detail-{row.id}">{row.detail}</span>
						{/if}
					</div>
					<div class="status">
						<Badge tone={row.tone} data-testid="badge-{row.id}">{row.badge}</Badge>
					</div>
					<div class="actions">
						{#if row.canRequest}
							<Button
								size="sm"
								data-testid="request-{row.id}"
								disabled={requesting === row.id}
								onclick={() => request(row)}
							>
								{requesting === row.id ? 'Asking…' : 'Request'}
							</Button>
						{/if}
						<Button
							size="sm"
							variant="ghost"
							data-testid="open-settings-{row.id}"
							onclick={() => void openSettingsPane(row.settingsUrl)}
						>
							Open settings
						</Button>
					</div>
				</div>
			{/each}
		</div>
	</section>

	<section class="card" id="plugins">
		<header>
			<span class="card-title">Plugins</span>
			<span class="spacer"></span>
			<a href="/plugins" data-testid="permissions-open-plugins">Open plugins</a>
		</header>
		<div class="body">
			<span class="hint">
				A plugin reaches Atlas only through the permissions its manifest asks for. Untick one
				and the plugin is refused it, whatever its manifest says.
			</span>
			{#if pluginRows.length === 0}
				<span class="hint" data-testid="no-plugin-grants">
					No installed plugin asks for a permission.
				</span>
			{/if}
			{#each pluginRows as plugin (plugin.id)}
				<div class="row plugin" data-testid="plugin-grants-{plugin.id}">
					<div class="what">
						<span class="name">{plugin.manifest?.name ?? plugin.id}</span>
						<span class="hint mono">{plugin.id}</span>
						{#if !plugin.enabled}
							<span class="hint">Disabled, so nothing runs whatever it holds.</span>
						{/if}
					</div>
					<div class="grants">
						{#each permissionChips(plugin) as chip (chip.permission)}
							{@const permission = chip.permission}
							<Checkbox
								label={permission}
								checked={chip.granted}
								disabled={togglingPlugin === plugin.id}
								data-testid="grant-{plugin.id}-{permission}"
								onchange={(e) => togglePermission(plugin, permission, e.currentTarget.checked)}
							/>
						{/each}
					</div>
				</div>
			{/each}
		</div>
	</section>

	<section class="card" id="agents">
		<header>
			<span class="card-title">Agent defaults</span>
			<span class="spacer"></span>
			<Button
				variant="primary"
				size="sm"
				data-testid="defaults-save"
				disabled={!defaultsDirty || savingDefaults}
				onclick={saveDefaults}
			>
				{savingDefaults ? 'Saving…' : 'Save'}
			</Button>
		</header>
		<div class="body">
			<span class="hint">Projects that leave a rule unset inherit these.</span>
			{#if defaults}
				{#each [{ rule: 'memory_writers', label: 'Memory writers', any: defaults.anyWriters }, { rule: 'task_movers', label: 'Task movers', any: defaults.anyMovers }] as const as group (group.rule)}
					<div class="rule" data-testid="default-{group.rule}">
						<span class="name">{group.label}</span>
						<Checkbox
							label="Any actor"
							checked={group.any}
							data-testid="any-{group.rule}"
							onchange={(e) => {
								if (!defaults) return;
								if (group.rule === 'memory_writers') defaults.anyWriters = e.currentTarget.checked;
								else defaults.anyMovers = e.currentTarget.checked;
							}}
						/>
						{#if !group.any}
							<div class="ticks">
								{#each defaults.actors as actor (actor)}
									<div class="tick">
										<Checkbox
											label={actor}
											checked={(group.rule === 'memory_writers'
												? defaults.memoryWriters
												: defaults.taskMovers)[actor] ?? false}
											onchange={(e) => {
												if (!defaults) return;
												const on = e.currentTarget.checked;
												if (group.rule === 'memory_writers') {
													defaults.memoryWriters = { ...defaults.memoryWriters, [actor]: on };
												} else {
													defaults.taskMovers = { ...defaults.taskMovers, [actor]: on };
												}
											}}
										/>
										{#if alwaysAllowed(actor)}<span class="hint">always allowed</span>{/if}
									</div>
								{/each}
							</div>
						{/if}
					</div>
				{/each}

				<div class="add">
					<Input
						placeholder="Add label"
						aria-label="Add label"
						mono
						bind:value={draft}
						onkeydown={(e) => {
							if (e.key === 'Enter') addActor();
						}}
					/>
					<Button size="sm" data-testid="defaults-add" disabled={!canAdd} onclick={addActor}>
						Add
					</Button>
				</div>

				<div class="rule" data-testid="default-require_review">
					<span class="name">Require review</span>
					<Checkbox
						label="Require review for memories proposed by agents"
						bind:checked={defaults.requireReview}
					/>
					<span class="hint">
						A project can raise this but not lower it: with the default on, review is required
						everywhere.
					</span>
				</div>
			{/if}
		</div>
	</section>
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

	.card {
		display: flex;
		flex-direction: column;
		flex: 0 0 auto;
	}

	header {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 32px;
		flex: 0 0 auto;
		padding: 0 12px;
		border-bottom: 1px solid var(--border-subtle);
	}

	.card-title {
		font-weight: 600;
	}

	.spacer {
		flex: 1;
	}

	.body {
		display: flex;
		flex-direction: column;
		gap: 12px;
		padding: 12px;
	}

	.row {
		display: flex;
		align-items: flex-start;
		gap: 12px;
	}

	.what {
		display: flex;
		flex-direction: column;
		gap: 2px;
		flex: 1;
		min-width: 0;
	}

	.name {
		font-weight: 600;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
		max-width: 80ch;
	}

	/* One column for the status, so every badge starts at the same x whatever its word. */
	.status {
		flex: 0 0 120px;
		display: flex;
		align-items: center;
	}

	.actions {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 6px;
		/* Wide enough for Request plus Open settings, so the status badge before it sits
		   at the same x on every row whether or not the row offers Request. */
		flex: 0 0 190px;
	}

	.plugin .grants {
		display: flex;
		flex-direction: column;
		gap: 4px;
		min-width: 200px;
	}

	.rule {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.ticks {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding-left: 16px;
	}

	.tick {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	/* An input and a button, not a bar: the button is its own width. */
	.add {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.add :global(.dbm-input) {
		width: 220px;
	}
</style>
