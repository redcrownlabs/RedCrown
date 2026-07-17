# Product Experience

Date: 2026-07-16
Status: Accepted

## Requirement

RedCrown needs Netflix-level polish without copying Netflix or Popcorn Time and
without sacrificing useful information for promotion-heavy presentation.

## Direction

- Original RedCrown visual language.
- Compact navigation and responsive poster grids.
- One clear `Watch now` action on details.
- No autoplay previews, giant hero takeover, or endless content rails.
- Settings expose only frequently useful choices.
- `Sources` makes ordered API fallback URLs first-class.

## Source editing invariants

- Every source has one compatible API contract and an ordered URL chain.
- Users can add, remove, reorder, enable, test, save, reset, and roll back URLs.
- Testing never replaces the active known-good chain.
- Empty valid results do not trigger failover.
- Endpoint credentials are rejected from URLs and secrets are not rendered.

## Accessibility and browser policy

The renderer targets the Chromium version bundled with Electron. Baseline
widely available features are used directly. Newer features require graceful
degradation and must not be essential to navigation, forms, or playback.

The UI uses semantic landmarks, native controls, visible focus, 200% text zoom,
reduced motion, stable artwork aspect ratios, and no hover-only actions.
# Watched-title discovery behavior

Home is a discovery surface, so completed movies are removed from its hero and
content rows using the durable library's provider identifiers. Watched movies
remain available in Movies, search results, and My Library. Series are not
hidden when individual episodes are watched because episode completion does not
mean the entire series is complete.

The renderer derives visible Home rows from raw catalog data and the current
library snapshot. It does not copy watched state into a second mutable store, so
importing watch history updates Home immediately.

Home carousels preserve native button activation for normal pointer clicks.
Pointer capture starts only after meaningful horizontal movement, and only a
confirmed drag suppresses navigation. This keeps cards keyboard-accessible and
prevents click-sized hand jitter from turning an item into a drag gesture.
