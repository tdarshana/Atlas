// Thin helpers over the vault commands (`src-tauri/src/commands/platform.rs`), shared by
// the global and per-project Extraction cards: both mirror a freshly typed key into the
// vault when it is unlocked, and both need to know whether it is.

import type { VaultStatus } from '$lib/types';
import { desktop } from './platform';

export async function vaultStatus(): Promise<VaultStatus> {
	return desktop('vault_status', undefined, () => 'missing' as VaultStatus);
}

/**
 * Mirrors `key` under `scope` (`"global"` or `"project/<id>"`). Callers check
 * `vaultStatus() === 'unlocked'` first; the Extraction cards show "Vault locked, key not
 * mirrored" rather than calling this while it is not.
 */
export async function mirrorKey(scope: string, key: string): Promise<void> {
	await desktop('vault_put_key', { scope, key }, () => undefined);
}
