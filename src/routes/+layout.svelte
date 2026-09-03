<script lang="ts">
	import { onMount, type Snippet } from 'svelte';
	import { page } from '$app/state';
	import '../app.css';
	import { boot, daemon } from '$lib/daemon.svelte';
	import {
		startStatusPolling,
		status,
		statusDetail,
		statusLabel
	} from '$lib/stores/status.svelte';
	import Button from '$lib/ui/Button.svelte';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import Toast from '$lib/ui/Toast.svelte';

	let { children }: { children: Snippet } = $props();

	const nav = [
		{ name: 'dashboard', href: '/', label: 'Dashboard' },
		{ name: 'projects', href: '/projects', label: 'Projects' },
		{ name: 'board', href: '/board', label: 'Board' },
		{ name: 'memories', href: '/memories', label: 'Memories' },
		{ name: 'agents', href: '/agents', label: 'Agents' },
		{ name: 'practices', href: '/practices', label: 'Practices' },
		{ name: 'workflows', href: '/workflows', label: 'Workflows' },
		{ name: 'review', href: '/review', label: 'Review' },
		{ name: 'settings', href: '/settings', label: 'Settings' }
	];

	function isActive(href: string): boolean {
		const path = page.url.pathname;
		return href === '/' ? path === '/' : path === href || path.startsWith(`${href}/`);
	}

	onMount(() => {
		void boot();
	});

	// Polling starts once the daemon answers and stops if the connection is lost.
	$effect(() => {
		if (!daemon.ready) return;
		return startStatusPolling();
	});
</script>

<div class="shell">
	<aside class="sidebar" data-testid="sidebar">
		<div class="brand">Atlas</div>
		<nav>
			{#each nav as item (item.href)}
				<a
					href={item.href}
					data-testid="nav-{item.name}"
					aria-current={isActive(item.href) ? 'page' : undefined}
				>
					{item.label}
				</a>
			{/each}
		</nav>
		<div class="foot">
			<span
				class="pill"
				class:offline={!!status.error || !!daemon.error}
				data-testid="status-pill"
				title={statusDetail()}
			>
				{daemon.error ? 'daemon offline' : statusLabel()}
			</span>
		</div>
	</aside>

	<main class="content">
		{#if daemon.error}
			<div class="fill" data-testid="daemon-error">
				<ErrorState message={daemon.error} logPath={daemon.logPath}>
					<Button variant="primary" onclick={() => boot()}>Retry</Button>
				</ErrorState>
			</div>
		{:else if !daemon.ready}
			<div class="fill"><p class="booting">Starting Atlas…</p></div>
		{:else}
			{@render children()}
		{/if}
	</main>
</div>

<Toast />

<style>
	.shell {
		display: grid;
		grid-template-columns: 210px 1fr;
		height: 100vh;
	}

	.sidebar {
		display: flex;
		flex-direction: column;
		gap: var(--space-4);
		padding: var(--space-4) var(--space-3);
		border-right: 1px solid var(--border);
		background: var(--bg-elev);
		overflow-y: auto;
	}

	.brand {
		padding: 0 var(--space-2);
		font-size: 16px;
		font-weight: 700;
		letter-spacing: 0.02em;
	}

	nav {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	nav a {
		padding: 6px var(--space-2);
		border-radius: var(--radius-sm);
		color: var(--muted);
		text-decoration: none;
	}

	nav a:hover {
		background: var(--bg-hover);
		color: var(--fg);
	}

	nav a[aria-current='page'] {
		background: var(--accent-soft);
		color: var(--accent);
		font-weight: 500;
	}

	.foot {
		margin-top: auto;
		padding: 0 var(--space-1);
	}

	.pill {
		display: inline-block;
		max-width: 100%;
		padding: 2px 8px;
		border: 1px solid var(--border);
		border-radius: 999px;
		background: var(--bg);
		color: var(--muted);
		font-size: 12px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.pill.offline {
		border-color: transparent;
		background: var(--danger-soft);
		color: var(--danger);
	}

	.content {
		padding: var(--space-5);
		overflow-y: auto;
	}

	.fill {
		display: flex;
		align-items: center;
		justify-content: center;
		height: 100%;
	}

	.booting {
		margin: 0;
		color: var(--muted);
	}
</style>
