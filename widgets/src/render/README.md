Pure/portable render layer — zero MCP imports, kept framework-free so a future local panel can reuse it unchanged.

- `polar.ts` — pure canvas radar renderer (zero React too).
- `overlays.ts` — pure canvas overlay painters for the W11 session timeline (density tracks, event markers, A/B cursors); zero React.
- `tooltip.ts` — shared floating tooltip for the chart surfaces: pure placement math + a thin DOM host (textContent-only rows, token-styled via `.ob-tooltip`), plus uPlot cursor/anchor coordinate helpers.
- `uplot-host.ts` — thin uPlot create/resize/destroy helper (DOM, no React): colors are passed to uPlot as functions reading CSS tokens at draw time (theme switch = redraw), overlay painters mount on the draw hook, and wheel-pan is added on top of uPlot's built-in drag-zoom.
