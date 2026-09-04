<script lang="ts">
	// Top-level navigation. Collapsed it is the DS 44px icon rail; expanded it is the
	// 176px labelled column from frame 01. Both sit on the window background with no right
	// border, as the design file overrides the DS rail to do.
	import { goto } from '$app/navigation';
	import { Icon } from '$lib/ds';
	import { shell, toggleRail } from './shell.svelte';
	import { MAIN_VIEWS, SETTINGS_VIEW, shortcutText, type ViewDef } from './views';

	function hint(v: ViewDef): string {
		return `${v.label}  ${shortcutText(v.combo, shell.platform)}`;
	}

	function open(v: ViewDef) {
		void goto(v.href);
	}
</script>

{#if shell.railExpanded}
	<nav class="expanded" aria-label="Views">
		<div class="head">
			<span class="heading">VIEWS</span>
		</div>
		{#each MAIN_VIEWS as v (v.id)}
			{@const active = shell.view === v.id}
			<button
				class="row"
				class:active
				type="button"
				aria-current={active ? 'page' : undefined}
				onclick={() => open(v)}
			>
				<Icon
					name={v.icon}
					size={16}
					color={active ? 'var(--accent)' : 'var(--text-tertiary)'}
				/>
				<span class="label">{v.label}</span>
				<span class="key">{shortcutText(v.combo, shell.platform)}</span>
			</button>
		{/each}
		<!-- Settings and the collapse control stay at the foot in both rail modes. -->
		<span class="spacer"></span>
		<button
			class="row"
			class:active={shell.view === SETTINGS_VIEW.id}
			type="button"
			aria-current={shell.view === SETTINGS_VIEW.id ? 'page' : undefined}
			onclick={() => open(SETTINGS_VIEW)}
		>
			<Icon
				name={SETTINGS_VIEW.icon}
				size={16}
				color={shell.view === SETTINGS_VIEW.id ? 'var(--accent)' : 'var(--text-tertiary)'}
			/>
			<span class="label">{SETTINGS_VIEW.label}</span>
			<span class="key">{shortcutText(SETTINGS_VIEW.combo, shell.platform)}</span>
		</button>
		<button class="row" type="button" title="Collapse rail" onclick={toggleRail}>
			<Icon name="chevrons-left" size={16} color="var(--text-tertiary)" />
			<span class="label">Collapse rail</span>
			<span class="key">{shortcutText('Mod+B', shell.platform)}</span>
		</button>
	</nav>
{:else}
	<nav class="dbm-rail" aria-label="Views">
		{#each MAIN_VIEWS as v (v.id)}
			<button
				class="dbm-rail__btn"
				class:dbm-rail__btn--active={shell.view === v.id}
				type="button"
				aria-label={v.label}
				aria-current={shell.view === v.id ? 'page' : undefined}
				title={hint(v)}
				onclick={() => open(v)}
			>
				<Icon name={v.icon} size={18} />
			</button>
		{/each}
		<span class="dbm-rail__spacer"></span>
		<button
			class="dbm-rail__btn"
			class:dbm-rail__btn--active={shell.view === SETTINGS_VIEW.id}
			type="button"
			aria-label={SETTINGS_VIEW.label}
			aria-current={shell.view === SETTINGS_VIEW.id ? 'page' : undefined}
			title={hint(SETTINGS_VIEW)}
			onclick={() => open(SETTINGS_VIEW)}
		>
			<Icon name={SETTINGS_VIEW.icon} size={18} />
		</button>
		<button
			class="dbm-rail__btn"
			type="button"
			aria-label="Expand rail"
			title="Expand rail  {shortcutText('Mod+B', shell.platform)}"
			onclick={toggleRail}
		>
			<Icon name="chevrons-right" size={18} />
		</button>
	</nav>
{/if}

<style>
	/* The design file drops the DS rail's right border and puts it on the window. */
	.dbm-rail {
		border-right: 0;
		background: var(--bg-base);
	}

	.expanded {
		display: flex;
		flex-direction: column;
		gap: 1px;
		width: 176px;
		flex: 0 0 176px;
		padding: 6px;
		background: var(--bg-base);
	}

	.head {
		display: flex;
		align-items: center;
		height: 28px;
		padding: 0 8px 4px;
		color: var(--text-tertiary);
	}

	.heading {
		font-size: 11px;
		font-weight: 600;
		letter-spacing: 0.04em;
	}

	.spacer {
		flex: 1;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 10px;
		height: 28px;
		flex: 0 0 28px;
		padding: 0 8px;
		border: 0;
		border-radius: 3px;
		background: transparent;
		color: var(--text-secondary);
		font-family: inherit;
		font-size: 12px;
		text-align: left;
		cursor: default;
		transition: var(--transition-hover);
	}

	.row:hover {
		background: var(--bg-hover);
		color: var(--text-primary);
	}

	.row.active {
		background: var(--accent-muted);
		color: var(--text-primary);
		box-shadow: inset 2px 0 0 var(--accent);
	}

	.label {
		flex: 1;
		font-weight: 500;
	}

	.key {
		font-family: var(--font-mono);
		font-size: 11px;
		color: var(--text-tertiary);
	}
</style>
