<script lang="ts">
	// The Notifications card: which kinds the background check may show, the OS permission
	// they need, and a test notification.
	import { Button, Checkbox } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { inTauri } from '$lib/shell';
	import { desktop } from '$lib/shell/platform';
	import { loadSettings, settings } from '$lib/stores/settings.svelte';
	import { push } from '$lib/platform/toasts.svelte';
	import SettingsCard from './SettingsCard.svelte';

	const notifyFlag = (key: string) => settings.values[key] === true;

	async function setNotifyFlag(key: string, value: boolean): Promise<void> {
		try {
			await api().setSettings({ [key]: value });
			await loadSettings();
		} catch (e) {
			push('error', errorMessage(e));
		}
	}

	/** `"granted"`, `"denied"`, `"default"` (not yet decided), or null before the first
	 * load; only `"default"` gets an "Allow notifications" button, since the other two
	 * are already decided one way or the other. */
	let notifyPermission = $state<string | null>(null);

	async function loadNotifyPermission(): Promise<void> {
		notifyPermission = await desktop('notification_permission', undefined, () => null);
	}

	async function requestNotifyPermission(): Promise<void> {
		notifyPermission = await desktop('notification_request_permission', undefined, () => notifyPermission);
	}

	const notifyPermissionHint = $derived(
		notifyPermission === 'granted'
			? 'Notifications are allowed.'
			: notifyPermission === 'denied'
				? 'Notifications are blocked in system settings.'
				: notifyPermission === 'default'
					? 'Notifications need permission before any of the above can show one.'
					: ''
	);

	let sendingTestNotification = $state(false);

	async function sendTestNotification(): Promise<void> {
		sendingTestNotification = true;
		try {
			await desktop('notify', { title: 'Atlas', body: 'This is a test notification.' }, async () => {
				if (typeof Notification === 'undefined') return;
				let permission = Notification.permission;
				if (permission === 'default') permission = await Notification.requestPermission();
				if (permission === 'granted') new Notification('Atlas', { body: 'This is a test notification.' });
			});
			push('success', 'Test notification sent');
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			sendingTestNotification = false;
		}
	}

	/** Re-reads the OS permission; the route calls it on load and on Reload. */
	export async function refresh(): Promise<void> {
		await loadNotifyPermission();
	}
</script>

<SettingsCard id="notifications" title="Notifications">
	{#snippet head()}
		<span class="spacer"></span>
		<Button
			variant="ghost"
			size="sm"
			data-testid="settings-test-notification"
			disabled={sendingTestNotification}
			onclick={sendTestNotification}
		>
			{sendingTestNotification ? 'Sending…' : 'Send test notification'}
		</Button>
	{/snippet}

	{#if inTauri() && notifyPermissionHint}
		<div class="recorder-row">
			<span class="hint" role="status" data-testid="settings-notify-permission">
				{notifyPermissionHint}
			</span>
			{#if notifyPermission === 'default'}
				<Button
					variant="ghost"
					size="sm"
					data-testid="settings-notify-request-permission"
					onclick={requestNotifyPermission}
				>
					Allow notifications
				</Button>
			{/if}
		</div>
	{/if}
	<Checkbox
		label="Memories waiting for review"
		checked={notifyFlag('ui.notify.review_pending')}
		data-testid="settings-notify-review-pending"
		onchange={() => setNotifyFlag('ui.notify.review_pending', !notifyFlag('ui.notify.review_pending'))}
		onreset={notifyFlag('ui.notify.review_pending') ? () => setNotifyFlag('ui.notify.review_pending', false) : undefined}
	/>
	<Checkbox
		label="Workflow runs finished or failed"
		checked={notifyFlag('ui.notify.workflow_runs')}
		data-testid="settings-notify-workflow-runs"
		onchange={() => setNotifyFlag('ui.notify.workflow_runs', !notifyFlag('ui.notify.workflow_runs'))}
		onreset={notifyFlag('ui.notify.workflow_runs') ? () => setNotifyFlag('ui.notify.workflow_runs', false) : undefined}
	/>
	<Checkbox
		label="Daemon unreachable"
		checked={notifyFlag('ui.notify.daemon_errors')}
		data-testid="settings-notify-daemon-errors"
		onchange={() => setNotifyFlag('ui.notify.daemon_errors', !notifyFlag('ui.notify.daemon_errors'))}
		onreset={notifyFlag('ui.notify.daemon_errors') ? () => setNotifyFlag('ui.notify.daemon_errors', false) : undefined}
	/>
	<span class="hint">
		A background check every 30 seconds; each kind is off until you turn it on.
	</span>
</SettingsCard>

<style>
	.spacer {
		flex: 1;
	}

	.hint {
		font-size: 11px;
		color: var(--text-tertiary);
	}

	.recorder-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}
</style>
