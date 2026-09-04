<script lang="ts">
	// Plugin management: what is installed, what each one may do, and the two ways to add
	// one. Enabling a plugin is what lets its code run at all, so the checkbox is the only
	// switch that matters here and an incompatible plugin's is disabled with its reason.
	import { onMount } from 'svelte';
	import { Badge, Checkbox, Input, Table, type TableColumn } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import {
		installFolder,
		installGithub,
		loadPlugins,
		plugins,
		setEnabled,
		uninstall
	} from '$lib/plugins/host.svelte';
	import { permissionChips } from '$lib/plugins/grants';
	import ToolChannelWarning from '$lib/plugins/ToolChannelWarning.svelte';
	import type { PluginInfo } from '$lib/plugins/types';
	import { inTauri, setStatusItems } from '$lib/shell';
	import Button from '$lib/ui/Button.svelte';
	import Dialog from '$lib/ui/Dialog.svelte';
	import EmptyState from '$lib/ui/EmptyState.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	/** `https://github.com/<owner>/<repo>` with an optional `/tree/<ref>`. */
	const GITHUB_URL = /^https:\/\/github\.com\/[\w.-]+\/[\w.-]+(\/tree\/[\w./-]+)?\/?$/;
	/** A path segment of nothing but dots is traversal, not an owner, a repo or a ref. */
	const DOT_SEGMENT = /(^|\/)\.{1,2}(\/|$)/;

	let busyId = $state<string | null>(null);
	let installing = $state(false);
	let githubOpen = $state(false);
	let githubUrl = $state('');
	let removing = $state<PluginInfo | null>(null);

	const enabledCount = $derived(plugins.items.filter((p) => p.enabled).length);
	const summary = $derived(`${plugins.items.length} installed, ${enabledCount} enabled`);
	const githubValid = $derived(
		GITHUB_URL.test(githubUrl.trim()) && !DOT_SEGMENT.test(githubUrl.trim())
	);

	const columns: TableColumn<PluginInfo>[] = [
		{ key: 'name', label: 'Name', width: '200px' },
		{ key: 'version', label: 'Version', width: '90px', mono: true },
		{ key: 'author', label: 'Author', width: '140px' },
		{ key: 'permissions', label: 'Permissions' },
		{ key: 'enabled', label: 'Enabled', width: '80px' },
		{ key: 'actions', label: '', width: '110px' }
	];

	/** What an install says. Installing is not consenting: the plugin lands disabled and
	 * nothing of it runs, so the toast points at the permission chips and the toggle
	 * rather than announcing a plugin that is already live. */
	function installedMessage(info: PluginInfo): string {
		return `Installed ${info.manifest?.name ?? info.id}. Review its permissions, then enable it.`;
	}

	async function pickFolder(): Promise<void> {
		if (!inTauri()) return;
		installing = true;
		try {
			const { open } = await import('@tauri-apps/plugin-dialog');
			const picked = await open({ directory: true });
			if (typeof picked !== 'string') return;
			const info = await installFolder(picked);
			push('success', installedMessage(info));
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			installing = false;
		}
	}

	async function installFromGithub(): Promise<void> {
		if (!githubValid) return;
		installing = true;
		try {
			const info = await installGithub(githubUrl.trim());
			push('success', installedMessage(info));
			githubOpen = false;
			githubUrl = '';
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			installing = false;
		}
	}

	async function toggle(plugin: PluginInfo): Promise<void> {
		busyId = plugin.id;
		try {
			await setEnabled(plugin.id, !plugin.enabled);
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			busyId = null;
		}
	}

	async function confirmRemove(): Promise<void> {
		const plugin = removing;
		if (!plugin) return;
		busyId = plugin.id;
		try {
			await uninstall(plugin.id);
			push('success', `Uninstalled ${plugin.manifest?.name ?? plugin.id}`);
			removing = null;
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			busyId = null;
		}
	}

	onMount(() => {
		void loadPlugins();
	});

	$effect(() => {
		setStatusItems({ right: [{ text: summary }] });
	});
</script>

<div class="title-row">
	<span class="title">Plugins</span>
	<span class="count" data-testid="plugins-summary">{summary}</span>
	<span class="spacer"></span>
	<Button
		variant="ghost"
		size="sm"
		data-testid="plugins-install-folder"
		disabled={!plugins.available || installing}
		onclick={pickFolder}
	>
		Install from folder…
	</Button>
	<Button
		variant="ghost"
		size="sm"
		data-testid="plugins-install-github"
		disabled={!plugins.available || installing}
		onclick={() => (githubOpen = true)}
	>
		Install from GitHub…
	</Button>
</div>

<div class="pane" data-testid="plugins-page">
	{#if !plugins.available}
		<EmptyState
			title="Plugins need the desktop app"
			hint="Installed plugins run in the Atlas window, not in a browser tab."
		/>
	{:else}
		{#if plugins.error}
			<p class="bad" role="alert" data-testid="plugins-error">{plugins.error}</p>
		{/if}
		<ToolChannelWarning />
		<div class="table">
			<Table
				id="plugins"
				{columns}
				rows={plugins.items}
				rowKey={(p: PluginInfo) => p.id}
			>
				{#snippet cell(plugin: PluginInfo, column: TableColumn<PluginInfo>)}
					{#if column.key === 'name'}
						<span class="name">
							<span>{plugin.manifest?.name ?? plugin.id}</span>
							{#if !plugin.compatible}
								<Badge tone="warning" title={plugin.reason ?? ''}>incompatible</Badge>
							{/if}
						</span>
					{:else if column.key === 'version'}
						{plugin.manifest?.version ?? '-'}
					{:else if column.key === 'author'}
						{plugin.manifest?.author ?? '-'}
					{:else if column.key === 'permissions'}
						<!-- The grant, not the request: a permission the user revoked on the
						     Permissions view stays on the row, greyed and struck through, so
						     this column says both what the plugin asked for and what it holds. -->
						<span class="chips">
							{#each permissionChips(plugin) as chip (chip.permission)}
								<Badge
									tone={chip.granted ? 'info' : 'neutral'}
									title={chip.granted ? '' : 'Asked for, revoked on the Permissions view'}
								>
									<span class:revoked={!chip.granted}>{chip.permission}</span>
								</Badge>
							{/each}
							{#if permissionChips(plugin).length === 0}
								<span class="hint">none</span>
							{/if}
						</span>
					{:else if column.key === 'enabled'}
						<Checkbox
							checked={plugin.enabled}
							disabled={!plugin.compatible || busyId === plugin.id}
							title={plugin.compatible ? '' : (plugin.reason ?? '')}
							aria-label={`Enable ${plugin.manifest?.name ?? plugin.id}`}
							data-testid={`plugin-toggle-${plugin.id}`}
							onchange={() => toggle(plugin)}
						/>
					{:else}
						<Button
							variant="ghost"
							size="sm"
							disabled={busyId !== null}
							data-testid={`plugin-uninstall-${plugin.id}`}
							onclick={() => (removing = plugin)}
						>
							Uninstall…
						</Button>
					{/if}
				{/snippet}
				{#snippet empty()}
					<span class="hint">No plugins installed yet. Install one from a folder or from GitHub.</span>
				{/snippet}
			</Table>
		</div>
		<span class="hint">
			A plugin runs in a sandboxed frame and reaches Atlas only through the permissions its
			manifest asks for, which are listed above before you enable it.
		</span>
	{/if}
</div>

<Dialog
	open={githubOpen}
	title="Install from GitHub"
	onclose={() => (githubOpen = false)}
>
	<div class="form">
		<Input
			label="Repository URL"
			placeholder="https://github.com/owner/repo"
			bind:value={githubUrl}
			data-testid="plugin-github-url"
		/>
		<span class="hint">
			A public repository, optionally at a branch or tag:
			<span class="mono">https://github.com/owner/repo/tree/v1.0.0</span>
		</span>
		{#if githubUrl.trim() !== '' && !githubValid}
			<span class="bad" data-testid="plugin-github-invalid">That is not a GitHub repository URL.</span>
		{/if}
	</div>
	{#snippet footer()}
		<Button variant="ghost" onclick={() => (githubOpen = false)}>Cancel</Button>
		<Button
			variant="primary"
			disabled={!githubValid || installing}
			data-testid="plugin-github-install"
			onclick={installFromGithub}
		>
			{installing ? 'Installing…' : 'Install'}
		</Button>
	{/snippet}
</Dialog>

<Dialog
	open={removing !== null}
	title="Uninstall plugin"
	onclose={() => (removing = null)}
>
	<p class="confirm">
		Remove <strong>{removing?.manifest?.name ?? removing?.id}</strong> and its files? Anything it
		contributed disappears with it.
	</p>
	{#snippet footer()}
		<Button variant="ghost" onclick={() => (removing = null)}>Cancel</Button>
		<Button
			variant="danger"
			disabled={busyId !== null}
			data-testid="plugin-uninstall-confirm"
			onclick={confirmRemove}
		>
			Uninstall
		</Button>
	{/snippet}
</Dialog>

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

	.count {
		color: var(--text-tertiary);
	}

	.spacer {
		flex: 1;
	}

	.pane {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 12px;
	}

	.table {
		border: 1px solid var(--border-subtle);
		border-radius: 3px;
		overflow: hidden;
	}

	.name {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}

	/* A permission the manifest asks for and the user has taken back. */
	.revoked {
		text-decoration: line-through;
		opacity: 0.7;
	}

	.form {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.confirm {
		margin: 0;
		font-size: 13px;
		color: var(--text-secondary);
	}

	.mono {
		font-family: var(--font-mono);
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.bad {
		margin: 0;
		color: var(--danger-text);
		font-size: 13px;
	}
</style>
