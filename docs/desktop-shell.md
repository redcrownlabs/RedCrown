# Desktop Shell and Browse Interaction

Date: 2026-07-16

RedCrown uses a frameless Electron window so the application navigation and
desktop chrome form one compact surface.

## Window chrome

- The renderer may request only minimize, maximize/restore, close, and current
  maximize state through the preload bridge.
- The main process verifies that every request originated from the primary
  `BrowserWindow`; arbitrary Electron APIs are never exposed to the renderer.
- The title bar is the only draggable region. Every interactive descendant is
  explicitly marked non-draggable so controls remain clickable.
- Maximize state is emitted by the main process so the renderer icon reflects
  external state changes such as Windows snap and keyboard shortcuts.

## Catalog rows

- Horizontal rows remain native scroll containers for keyboard, wheel, touch,
  and assistive-technology compatibility.
- The visual scrollbar is hidden because explicit directional controls and edge
  fades communicate overflow.
- Pointer dragging changes only `scrollLeft`; crossing the drag threshold
  suppresses the card click that would otherwise open a title accidentally.
- Row content, headings, hero copy, and controls share one application gutter.
  This prevents cards from touching the window edge at narrow widths.

## Catalog controls

Catalog filters use semantic `<select>` controls with Chromium's customizable
`base-select` appearance. Electron's pinned Chromium runtime provides the
top-layer picker and keyboard/accessibility contract while RedCrown supplies
the visual design. This avoids a JavaScript listbox implementation and its
additional focus-management failure modes.

Pagination is an internal provider concern. A non-interactive sentinel requests
the next page before the user reaches the grid end. A synchronous ref guard
prevents duplicate requests, and a query generation rejects stale responses
after category, sort, genre, or search changes. The UI exposes no button, page
number, page size, or page status. A failed request stops instead of retrying in
a loop and surfaces the ordinary catalog error.
