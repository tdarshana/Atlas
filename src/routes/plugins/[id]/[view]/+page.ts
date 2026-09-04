// A plugin's section view. Two dynamic segments have no crawlable entries, so the static
// adapter's index.html fallback serves this route at runtime, the way the project hub's
// own dynamic route already does.

import type { PageLoad } from './$types';

export const prerender = false;

export const load: PageLoad = ({ params }) => ({ id: params.id, view: params.view });
