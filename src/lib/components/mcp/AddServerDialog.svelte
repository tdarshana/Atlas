<script lang="ts">
	// Adding an MCP server to one of the agents' own config files. The Agent and scope
	// select says which file the entry lands in; the rest is the entry itself. Secret
	// values are sent once and never come back: the list only ever carries key names.
	import { Button, Input, Select } from '$lib/ds';
	import { errorMessage } from '$lib/errors';
	import { addTargets, nameError, pairsToObject, splitArgs } from '$lib/mcp-servers';
	import type { NewMcpServer, Uuid } from '$lib/types';
	import Dialog from '$lib/ui/Dialog.svelte';
	import Textarea from '$lib/ui/Textarea.svelte';

	interface Props {
		open: boolean;
		/** Null offers only the user-level targets; an id also offers this project's files
		 * and defaults to its Claude Code project scope. */
		projectId: Uuid | null;
		onclose: () => void;
		onadd: (input: NewMcpServer) => Promise<unknown>;
	}

	let { open, projectId, onclose, onadd }: Props = $props();

	const targets = $derived(addTargets(!!projectId));

	let target = $state('');
	let name = $state('');
	let kind = $state<'stdio' | 'http'>('stdio');
	let command = $state('');
	let args = $state('');
	let url = $state('');
	let pairs = $state<{ key: string; value: string }[]>([{ key: '', value: '' }]);
	let saving = $state(false);
	let error = $state<string | null>(null);

	const invalid = $derived(
		nameError(name) ??
			(kind === 'stdio'
				? command.trim() === ''
					? 'A stdio server needs a command.'
					: null
				: url.trim() === ''
					? 'An HTTP server needs a URL.'
					: null)
	);

	$effect(() => {
		if (!open) return;
		target = targets[0]?.value ?? '';
		name = '';
		kind = 'stdio';
		command = '';
		args = '';
		url = '';
		pairs = [{ key: '', value: '' }];
		error = null;
	});

	function setPair(index: number, field: 'key' | 'value', value: string): void {
		pairs = pairs.map((pair, i) => (i === index ? { ...pair, [field]: value } : pair));
	}

	async function submit(): Promise<void> {
		if (invalid) {
			error = invalid;
			return;
		}
		const chosen = targets.find((t) => t.value === target);
		if (!chosen) {
			error = 'Pick where the server should be added.';
			return;
		}

		saving = true;
		try {
			await onadd({
				source: chosen.source,
				scope: chosen.scope,
				project_id: chosen.needsProject ? projectId : null,
				name: name.trim(),
				transport:
					kind === 'stdio'
						? {
								kind: 'stdio',
								command: command.trim(),
								args: splitArgs(args),
								env: pairsToObject(pairs)
							}
						: { kind: 'http', url: url.trim(), headers: pairsToObject(pairs) }
			});
			onclose();
		} catch (e) {
			error = errorMessage(e);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog {open} title="Add MCP server" {onclose}>
	<div class="form">
		<Select
			label="Agent and scope"
			options={targets.map((t) => ({ value: t.value, label: t.label }))}
			bind:value={target}
			data-testid="add-server-target"
		/>
		<Input
			label="Name"
			mono
			placeholder="filesystem"
			bind:value={name}
			data-testid="add-server-name"
		/>

		<div class="row" role="radiogroup" aria-label="Transport">
			<Button
				size="sm"
				variant={kind === 'stdio' ? 'primary' : 'ghost'}
				data-testid="add-server-kind-stdio"
				onclick={() => (kind = 'stdio')}
			>
				stdio
			</Button>
			<Button
				size="sm"
				variant={kind === 'http' ? 'primary' : 'ghost'}
				data-testid="add-server-kind-http"
				onclick={() => (kind = 'http')}
			>
				HTTP
			</Button>
		</div>

		{#if kind === 'stdio'}
			<Input
				label="Command"
				mono
				placeholder="npx"
				bind:value={command}
				data-testid="add-server-command"
			/>
			<div class="group">
				<span class="group-heading">Arguments</span>
				<Textarea
					bind:value={args}
					mono
					rows={4}
					aria-label="Arguments"
					placeholder={'-y\n@modelcontextprotocol/server-filesystem'}
					data-testid="add-server-args"
				/>
				<span class="hint">One argument per line.</span>
			</div>
		{:else}
			<Input
				label="URL"
				mono
				placeholder="https://mcp.example.com/sse"
				bind:value={url}
				data-testid="add-server-url"
			/>
		{/if}

		<div class="group">
			<span class="group-heading">{kind === 'stdio' ? 'Environment' : 'Headers'}</span>
			{#each pairs as pair, index (index)}
				<div class="row">
					<Input
						mono
						placeholder="KEY"
						aria-label="Key"
						value={pair.key}
						oninput={(e) => setPair(index, 'key', e.currentTarget.value)}
						data-testid="add-server-key-{index}"
					/>
					<Input
						mono
						type="password"
						placeholder="value"
						aria-label="Value"
						value={pair.value}
						oninput={(e) => setPair(index, 'value', e.currentTarget.value)}
						data-testid="add-server-value-{index}"
					/>
				</div>
			{/each}
			<div class="row">
				<Button
					size="sm"
					variant="ghost"
					data-testid="add-server-add-pair"
					onclick={() => (pairs = [...pairs, { key: '', value: '' }])}
				>
					Add row
				</Button>
			</div>
			<span class="hint">
				Values are written into the agent's config file and never read back; Atlas only ever
				shows the key names.
			</span>
		</div>

		{#if error}<p class="bad" role="alert" data-testid="add-server-error">{error}</p>{/if}
	</div>

	{#snippet footer()}
		<span class="spacer"></span>
		<Button variant="ghost" size="sm" onclick={onclose}>Cancel</Button>
		<Button
			variant="primary"
			size="sm"
			disabled={saving || !!invalid}
			data-testid="add-server-submit"
			onclick={submit}
		>
			{saving ? 'Adding…' : 'Add'}
		</Button>
	{/snippet}
</Dialog>

<style>
	.form {
		display: flex;
		flex-direction: column;
		gap: 10px;
		min-width: 520px;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.row :global(.dbm-input) {
		flex: 1;
		min-width: 0;
	}

	.group {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}

	.spacer {
		flex: 1;
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
