<script lang="ts">
	// The About card's head buttons: Copy diagnostics and Open log folder. The diagnostics
	// text folds the persisted UI state (`ui_state_all`) in with the about info, since a
	// bug report often hinges on which rail, theme or filters were active.
	import { Button } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { copyText, inTauri } from '$lib/shell';
	import { desktop } from '$lib/shell/platform';
	import { openLogFolder } from '$lib/search/commands';
	import { status } from '$lib/stores/status.svelte';
	import type { AboutInfo } from '$lib/types';
	import { push } from '$lib/platform/toasts.svelte';
	import { diagnosticsText } from './diagnostics';

	interface Props {
		about: AboutInfo | null;
	}

	let { about }: Props = $props();

	let uiState = $state<Record<string, unknown>>({});

	async function loadUiState(): Promise<void> {
		uiState = await desktop('ui_state_all', undefined, () => ({}));
	}

	const diagnostics = $derived(
		about
			? diagnosticsText(
					{
						...about,
						daemon_version: status.report?.version ?? 'unknown',
						db_path: status.report?.db_path ?? 'unknown'
					},
					uiState
				)
			: ''
	);

	/** How long the button says "Copied" before going back to its own name. */
	const COPIED_MS = 1500;
	let copied = $state<string | null>(null);
	let copiedTimer: ReturnType<typeof setTimeout> | null = null;

	/**
	 * Copies a snippet and names which one was copied, so the feedback is on the button.
	 * The label goes back to "Copy" shortly after: a button that stays "Copied" reads as
	 * its permanent name, and gives no feedback the second time it is pressed.
	 */
	async function copy(label: string, text: string): Promise<void> {
		try {
			await copyText(text);
			copied = label;
			if (copiedTimer !== null) clearTimeout(copiedTimer);
			copiedTimer = setTimeout(() => {
				copiedTimer = null;
				copied = null;
			}, COPIED_MS);
		} catch (e) {
			push('error', `Could not copy: ${errorMessage(e)}`);
		}
	}

	$effect(() => () => {
		if (copiedTimer !== null) clearTimeout(copiedTimer);
	});

	/** Re-reads the persisted UI state; the About card calls it on load and on Reload. */
	export async function refresh(): Promise<void> {
		await loadUiState();
	}
</script>

<Button
	variant="ghost"
	size="sm"
	data-testid="settings-copy-diagnostics"
	disabled={!about}
	onclick={() => copy('diagnostics', diagnostics)}
>
	{copied === 'diagnostics' ? 'Copied' : 'Copy diagnostics'}
</Button>
<Button
	variant="ghost"
	size="sm"
	data-testid="settings-open-log-folder"
	disabled={!inTauri()}
	title={inTauri() ? 'Reveal the log folder' : 'Only available in the desktop app'}
	onclick={openLogFolder}
>
	Open log folder
</Button>
