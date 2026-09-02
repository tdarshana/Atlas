// Agent names are not known at build time, so the static adapter must not try to
// prerender this route; the SPA fallback serves it. The editor sits under `edit/`
// rather than `/agents/[name]` so `/agents/new` cannot shadow an agent called
// "new", which is a legal name.
export const prerender = false;
