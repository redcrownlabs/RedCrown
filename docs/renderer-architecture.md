# Renderer architecture

Date: 2026-07-18
Status: Accepted

## Requirement

The desktop renderer must remain easy to extend as individual product areas grow.
The previous renderer placed navigation, every screen, shared controls, and
feature-specific helpers in one root-level `App.tsx`, which obscured ownership
and made unrelated UI changes collide.

## Decision

Renderer source is organized by responsibility:

- `app/` owns application composition, navigation, and cross-feature state.
- `features/<name>/` owns a user-facing area, its components, pure models, and
  colocated tests.
- `shared/` owns generated renderer contracts, the typed IPC entry point, and
  UI primitives used by multiple features.
- Root-level files are limited to renderer bootstrapping, ambient declarations,
  and the global stylesheet.

Imports use concrete module paths instead of feature or UI barrel files. This
keeps the dependency graph statically analyzable and makes component ownership
visible at each call site.

## Invariants

- Feature modules do not import from `app/`.
- Shared modules contain no feature-specific behavior.
- Pure behavior is kept outside components and tested beside the owning feature.
- `App` coordinates screens but does not implement their presentation.
- Generated IPC projections remain the single renderer contract boundary.

## Tradeoff

The global stylesheet remains a single file because the renderer currently has
one theme and its selectors already form a coherent design layer. Splitting it
mechanically would add import ordering concerns without establishing meaningful
style ownership. It should be split only when feature-level style isolation or
CSS modules are adopted deliberately.
