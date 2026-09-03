// Thin helpers over the vault commands (`src-tauri/src/commands/platform.rs`), shared by
// the global and per-project Extraction cards: both mirror a freshly typed key into the
// vault when it is unlocked, and both need to know whether it is.

import type { VaultStatus } from '$lib/types';
import { desktop } from './platform';

export async function vaultStatus(): Promise<VaultStatus> {
	return desktop('vault_status', undefined, () => 'missing' as VaultStatus);
}

/**
 * The line an Extraction card shows under the API key field for a vault that will not
 * mirror it: `null` once it is unlocked, since there is nothing to warn about. Distinct
 * sentences for "missing" and "locked" so a fresh install and a locked vault do not read
 * the same.
 */
export function vaultHintText(status: VaultStatus): string | null {
	if (status === 'missing') return 'No vault yet, key not mirrored.';
	if (status === 'locked') return 'Vault locked, key not mirrored.';
	return null;
}

/**
 * Mirrors `key` under `scope` (`"global"` or `"project/<id>"`). Callers check
 * `vaultStatus() === 'unlocked'` first; the Extraction cards show "Vault locked, key not
 * mirrored" rather than calling this while it is not.
 */
export async function mirrorKey(scope: string, key: string): Promise<void> {
	await desktop('vault_put_key', { scope, key }, () => undefined);
}
