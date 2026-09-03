<script lang="ts">
	// The Log tab: this project's event history, newest first, behind a source, an event
	// kind and a free-text filter. Frame 02.6.
	import { untrack } from 'svelte';
	import { page } from '$app/state';
	import { Badge, Button, Input, Select, Table } from '$lib/ds';
	import type { TableColumn } from '$lib/ds';
	import { api } from '$lib/daemon.svelte';
	import { errorMessage } from '$lib/errors';
	import { logTime, plural } from '$lib/format';
	import { inTauri, setStatusItems } from '$lib/shell';
	import { project, setHeaderActions } from '$lib/stores/project.svelte';
	import {
		cancelLoadLog,
		clearLogFilters,
		hasFilters,
		LOG_DEBOUNCE_MS,
		loadLog,
		loadMoreLog,
		log,
		openLog,
		scheduleLoadLog
	} from '$lib/stores/log.svelte';
	import {
		eventsToday,
		exportFilename,
		kindOptions,
		refHref,
		refLabel,
		sourceOptions
	} from '$lib/components/project/log/log';
	import type { LogEntry } from '$lib/types';
	import ErrorState from '$lib/ui/ErrorState.svelte';
	import { push } from '$lib/ui/toasts.svelte';

	/** The daemon's rows carry no id, so the table is keyed by position instead. */
	type Row = LogEntry & { rowId: string };

	let exporting = $state(false);

	const id = $derived(page.params.id ?? '');
	const name = $derived(project.current?.name ?? 'Project');
	const rows = $derived<Row[]>(log.rows.map((entry, i) => ({ ...entry, rowId: String(i) })));
	const sources = $derived(sourceOptions(log.sources, log.source));
	const kinds = $derived(kindOptions(log.kinds, log.kind));
	const filtered = $derived(hasFilters(log));

	// Neither column is sortable: the daemon answers newest first and pages by the last
	// row's time, so re-ordering the page would break the row `Load more` asks after.
	const columns: TableColumn<Row>[] = [
		{ key: 'time', label: 'Time', width: '100px' },
		{ key: 'source', label: 'Source', width: '180px' },
		{ key: 'event', label: 'Event', width: '120px' },
		{ key: 'detail', label: 'Detail' },
		{ key: 'ref', label: 'Ref', width: '110px', align: 'right' }
	];

	// Only the route id belongs in this effect's dependencies. `loadLog` reads every filter
	// on its way past, and tracking those would turn each keystroke into a second request.
	$effect(() => {
		const pid = id;
		if (!pid) return;
		untrack(() => {
			openLog(pid);
			void loadLog();
		});
		return cancelLoadLog;
	});

	$effect(() => {
		setStatusItems({
			right: [{ text: `${name} · ${plural(eventsToday(log.rows), 'event')} today` }]
		});
	});

	$effect(() => {
		setHeaderActions(headerActions);
		return () => setHeaderActions(null);
	});

	/**
	 * The whole log as JSONL. Inside the app the user picks the path and the file plugin
	 * writes it; in a browser there is no path to pick, so the same text goes out as a
	 * download instead.
	 */
	async function exportLog() {
		if (!id || exporting) return;
		exporting = true;
		try {
			const text = await api().projectLogExport(id);
			const filename = exportFilename(name);
			if (inTauri()) {
				const { save } = await import('@tauri-apps/plugin-dialog');
				const path = await save({ defaultPath: filename });
				if (!path) return;
				const { writeTextFile } = await import('@tauri-apps/plugin-fs');
				await writeTextFile(path, text);
				push('success', `Log written to ${path}`);
			} else {
				download(filename, text);
				push('success', `Exported ${filename}`);
			}
		} catch (e) {
			push('error', errorMessage(e));
		} finally {
			exporting = false;
		}
	}

	function download(filename: string, text: string): void {
		const url = URL.createObjectURL(new Blob([text], { type: 'application/x-ndjson' }));
		const a = document.createElement('a');
		a.href = url;
		a.download = filename;
		a.click();
		URL.revokeObjectURL(url);
	}
</script>

{#snippet headerActions()}
	{#if filtered}
		<Button variant="ghost" data-testid="log-clear" onclick={clearLogFilters}>Clear filters</Button>
	{/if}
	<Button variant="ghost" data-testid="log-export" disabled={exporting} onclick={exportLog}>
		{exporting ? 'Exporting…' : 'Export…'}
	</Button>
{/snippet}

<div class="filters">
	<!-- Set then load, rather than `bind:value` plus `onchange`: with a spread handler and a
	     binding on the same select the order the two run in is not the caller's to choose,
	     and a load that read the old value would filter by the previous source. -->
	<div class="w150">
		<Select
			options={sources}
			aria-label="Source"
			value={log.source}
			onchange={(e) => {
				log.source = e.currentTarget.value;
				void loadLog();
			}}
		/>
	</div>
	<div class="w150">
		<Select
			options={kinds}
			aria-label="Event"
			value={log.kind}
			onchange={(e) => {
				log.kind = e.currentTarget.value;
				void loadLog();
			}}
		/>
	</div>
	<div class="w300">
		<Input
			placeholder="Filter log…"
			aria-label="Filter log"
			bind:value={log.q}
			oninput={() => scheduleLoadLog(LOG_DEBOUNCE_MS)}
		/>
	</div>
</div>

{#if log.error}
	<ErrorState message={log.error} logPath={log.errorLogPath ?? undefined}>
		<Button variant="primary" onclick={() => void loadLog()}>Retry</Button>
	</ErrorState>
{:else}
	<div class="card sheet" data-testid="log-table">
		<div class="scroll">
			<Table
				id="project-log"
				{columns}
				{rows}
				rowKey={(row: Row) => row.rowId}
				emptyText={log.loading ? 'Loading…' : 'No events yet'}
			>
				{#snippet cell(row: Row, column: TableColumn<Row>)}
					{#if column.key === 'time'}
						<span class="mono time">{logTime(row.time)}</span>
					{:else if column.key === 'source'}
						<span class="mono who">{row.source}</span>
					{:else if column.key === 'event'}
						<Badge variant="outline">{row.kind}</Badge>
					{:else if column.key === 'detail'}
						<span class="detail" title={row.detail}>{row.detail}</span>
					{:else if row.ref}
						{@const href = refHref(row, id)}
						{#if href}
							<a class="mono ref" {href}>{refLabel(row)}</a>
						{:else}
							<span class="mono ref">{refLabel(row)}</span>
						{/if}
					{/if}
				{/snippet}
			</Table>

			{#if log.more}
				<div class="more">
					<Button
						data-testid="log-more"
						disabled={log.loadingMore}
						onclick={() => void loadMoreLog()}
					>
						{log.loadingMore ? 'Loading…' : 'Load more'}
					</Button>
				</div>
			{/if}
		</div>
	</div>
{/if}

<style>
	.filters {
		display: flex;
		align-items: center;
		gap: 8px;
		flex: 0 0 auto;
	}

	.w150 {
		width: 150px;
	}

	.w300 {
		width: 300px;
	}

	.sheet {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
		overflow: hidden;
	}

	.scroll {
		flex: 1;
		min-height: 0;
		overflow: auto;
	}

	.time {
		color: var(--text-tertiary);
	}

	.who {
		font-weight: 600;
	}

	.detail {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--text-secondary);
	}

	.ref {
		color: var(--accent);
	}

	.more {
		display: flex;
		justify-content: center;
		padding: 8px;
		border-top: 1px solid var(--border-subtle);
	}
</style>
