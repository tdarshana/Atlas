// The workflow editor's route shape. A dynamic segment has no crawlable entries, so the
// static adapter's index.html fallback serves every tab at runtime. The workflow itself
// is fetched by the layout component once the daemon has answered, mirroring the project
// hub's own `+layout.ts` for the same reason: the port is only known after `boot()`.

import type { LayoutLoad } from './$types';

export const prerender = false;

export const load: LayoutLoad = ({ params }) => ({ id: params.id });
