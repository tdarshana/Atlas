<script lang="ts">
	// The global stage list, which every project without an override follows.
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import StageEditor from '$lib/components/StageEditor.svelte';
	import type { Stage } from '$lib/types';
	import { push } from '$lib/platform/toasts.svelte';
	import SettingsCard from './SettingsCard.svelte';

	let stages = $state<Stage[]>([]);
	let stagesError = $state<string | null>(null);

	async function loadStages(): Promise<void> {
		try {
			stages = (await api().boardStages()).stages;
			stagesError = null;
		} catch (e) {
			stages = [];
			stagesError = errorMessage(e);
		}
	}

	async function saveStages(next: Stage[], renames: Record<string, string>): Promise<void> {
		try {
			stages = await api().setBoardStages(next, renames);
			stagesError = null;
			push('success', 'Board stages saved');
		} catch (e) {
			stagesError = errorMessage(e);
			push('error', stagesError);
		}
	}

	/** Re-reads the stage list; the route calls it on load and on Reload. */
	export async function refresh(): Promise<void> {
		await loadStages();
	}
</script>

<SettingsCard id="board-stages" title="Board stages">
	<span class="hint">
		The columns every project uses unless it sets its own. Renaming a stage here moves
		the tasks standing in it.
	</span>
	{#if stagesError}
		<p class="bad" role="alert" data-testid="board-stages-error">{stagesError}</p>
	{/if}
	<StageEditor {stages} onsave={saveStages} />
</SettingsCard>

<style>
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
