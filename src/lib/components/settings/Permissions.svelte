<script lang="ts">
	// The Permissions card: the system permission rows, read once for the card's count.
	// The Permissions view itself polls; a summary line does not need to.
	import { onMount } from 'svelte';
	import { permissionsStatus } from '$lib/permissions/commands';
	import { summaryText, toRows, type PermissionStatus } from '$lib/permissions/system';
	import { asksForPermissions } from '$lib/plugins/grants';
	import { plugins } from '$lib/plugins/host.svelte';
	import { loadProjects, projects } from '$lib/stores/projects.svelte';
	import SettingsCard from './SettingsCard.svelte';

	let permissionStatuses = $state<PermissionStatus[]>([]);

	const permissionsSummaryText = $derived(
		summaryText(
			toRows(permissionStatuses),
			plugins.items.filter(asksForPermissions).length
		)
	);

	async function loadPermissionStatuses(): Promise<void> {
		try {
			permissionStatuses = await permissionsStatus(
				projects.items.map((p) => p.root_path).filter(Boolean)
			);
		} catch {
			// A status read that fails leaves the card counting nothing granted, which is
			// what an app that cannot ask the OS actually knows.
		}
	}

	onMount(() => {
		// The files row probes the connected roots, so the project list has to be in hand
		// before the status read means anything.
		void (projects.items.length === 0 ? loadProjects() : Promise.resolve()).then(
			loadPermissionStatuses
		);
	});
</script>

<SettingsCard id="permissions" title="Permissions">
	<span class="hint" data-testid="permissions-settings-summary">{permissionsSummaryText}</span>
	<span class="hint">
		The macOS permissions Atlas needs, what each installed plugin may do, and the agent
		access defaults every project inherits, all on their own view.
	</span>
	<a href="/permissions" data-testid="permissions-open-link">Open permissions</a>
</SettingsCard>

<style>
	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
