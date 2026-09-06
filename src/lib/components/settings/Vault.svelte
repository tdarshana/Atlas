<script lang="ts">
	// The Security card: the encrypted vault (`atlas.hold`) that mirrors every extraction
	// API key saved on this screen. The route's Save flow needs to know whether the vault
	// is unlocked, so every status read is reported through `onstatus`.
	import { Badge, Button, Input } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { inTauri } from '$lib/shell';
	import { desktop } from '$lib/shell/platform';
	import { vaultStatus } from '$lib/shell/vault';
	import type { VaultStatus } from '$lib/types';
	import { push } from '$lib/platform/toasts.svelte';
	import SettingsCard from './SettingsCard.svelte';

	interface Props {
		/** Called with the status after every read, so the Extraction card can follow it. */
		onstatus?: (status: VaultStatus) => void;
	}

	let { onstatus }: Props = $props();

	let vault = $state<VaultStatus>('missing');
	let vaultPassphrase = $state('');
	let vaultBusy = $state(false);
	let vaultError = $state<string | null>(null);
	let vaultScopes = $state<string[]>([]);

	async function loadVault(): Promise<void> {
		vault = await vaultStatus();
		vaultScopes = vault === 'unlocked' ? await desktop('vault_list', undefined, () => []) : [];
		onstatus?.(vault);
	}

	async function setVaultPassphrase(): Promise<void> {
		vaultBusy = true;
		vaultError = null;
		try {
			await desktop('vault_set_passphrase', { passphrase: vaultPassphrase }, () => undefined);
			vaultPassphrase = '';
			await loadVault();
			push('success', 'Vault passphrase set');
		} catch (e) {
			vaultError = errorMessage(e);
		} finally {
			vaultBusy = false;
		}
	}

	async function unlockVault(): Promise<void> {
		vaultBusy = true;
		vaultError = null;
		try {
			await desktop('vault_unlock', { passphrase: vaultPassphrase }, () => undefined);
			vaultPassphrase = '';
			await loadVault();
			push('success', 'Vault unlocked');
		} catch (e) {
			vaultError = errorMessage(e);
		} finally {
			vaultBusy = false;
		}
	}

	async function lockVault(): Promise<void> {
		vaultBusy = true;
		vaultError = null;
		try {
			await desktop('vault_lock', undefined, () => undefined);
			await loadVault();
		} catch (e) {
			vaultError = errorMessage(e);
		} finally {
			vaultBusy = false;
		}
	}

	async function reapplyVaultKey(scope: string): Promise<void> {
		vaultBusy = true;
		vaultError = null;
		try {
			await desktop('vault_reapply', { scope }, () => undefined);
			push('success', `Reapplied the key for ${scope}`);
		} catch (e) {
			vaultError = errorMessage(e);
		} finally {
			vaultBusy = false;
		}
	}

	/** Re-reads the vault status and its scopes; the route calls it on load, on Reload
	 * and after it mirrors a freshly saved key. */
	export async function refresh(): Promise<void> {
		await loadVault();
	}
</script>

<SettingsCard id="security" title="Security">
	{#snippet head()}
		<span class="spacer"></span>
		<Badge tone={vault === 'unlocked' ? 'success' : vault === 'locked' ? 'warning' : 'neutral'}>
			{vault === 'missing' ? 'no vault' : vault}
		</Badge>
	{/snippet}

	<span class="hint">
		An encrypted vault (<span class="mono">atlas.hold</span>) in the app data folder that
		mirrors every extraction API key you save here. The daemon keeps its own copy in
		DuckDB, so extraction still works headless without the vault unlocked.
	</span>
	<Input
		label="Passphrase"
		type="password"
		autocomplete="off"
		bind:value={vaultPassphrase}
		disabled={!inTauri() || vaultBusy}
		data-testid="vault-passphrase"
	/>
	<div class="recorder-row">
		{#if vault === 'missing'}
			<Button
				variant="primary"
				size="sm"
				data-testid="vault-set"
				disabled={!inTauri() || vaultBusy || !vaultPassphrase}
				onclick={setVaultPassphrase}
			>
				{vaultBusy ? 'Setting…' : 'Set vault passphrase'}
			</Button>
		{:else if vault === 'locked'}
			<Button
				variant="primary"
				size="sm"
				data-testid="vault-unlock"
				disabled={!inTauri() || vaultBusy || !vaultPassphrase}
				onclick={unlockVault}
			>
				{vaultBusy ? 'Unlocking…' : 'Unlock vault'}
			</Button>
		{:else}
			<Button size="sm" data-testid="vault-lock" disabled={vaultBusy} onclick={lockVault}>
				{vaultBusy ? 'Locking…' : 'Lock'}
			</Button>
		{/if}
	</div>
	{#if vaultError}
		<p class="bad" role="alert" data-testid="vault-error">{vaultError}</p>
	{/if}
	{#if vault === 'unlocked'}
		<div class="group">
			<span class="group-heading">Stored keys</span>
			{#if vaultScopes.length === 0}
				<span class="hint">No keys mirrored yet.</span>
			{:else}
				{#each vaultScopes as scope (scope)}
					<div class="transport">
						<span class="mono value">{scope}</span>
						<span class="spacer"></span>
						<Button
							variant="ghost"
							size="sm"
							data-testid={`vault-reapply-${scope}`}
							disabled={vaultBusy}
							onclick={() => reapplyVaultKey(scope)}
						>
							Reapply
						</Button>
					</div>
				{/each}
			{/if}
		</div>
	{/if}
</SettingsCard>

<style>
	.group {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.transport {
		display: flex;
		align-items: center;
		gap: 12px;
		height: 22px;
		color: var(--text-secondary);
	}

	.value {
		color: var(--text-primary);
	}

	.spacer {
		flex: 1;
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

	.recorder-row {
		display: flex;
		align-items: center;
		gap: 8px;
	}
</style>
