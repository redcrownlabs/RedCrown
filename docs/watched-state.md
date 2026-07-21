# Watched state and new episodes

Date: 2026-07-21
Status: Accepted

## Requirement

Users must be able to mark a movie or one selected series episode as watched,
including content they watched outside RedCrown. Continue Watching must offer
the first watchable regular episode not explicitly marked watched. Repeated
visible buttons are avoided: media cards expose the same actions through one
custom context menu on Home, catalog pages, title artwork, and My Library.

## Decision

Movie watched state is stored directly against the canonical catalog identity.
For a series, each mutation targets exactly the episode selected in the detail
view. The library summary retains every exact episode identity and also projects
the highest watched season/episode for ordering and diagnostics. Home compares
the exact watched set with a freshly loaded episode list and adds the first
unwatched, watchable episode to Continue Watching.

The Continue Watching card keeps the current catalog metadata when available,
falls back to metadata imported from Popcorn Time, and opens the exact newer
episode instead of resetting selection to the first episode.

The card context menu provides a separate title-level series operation. It
loads the current catalog episode snapshot and marks every known regular
episode watched in one transaction. This does not weaken the detail view's
single-selected-episode contract.

Removing a series from Continue Watching stores a dedicated suppression flag.
It never changes watched episodes or favorites. The same context menu can
restore a suppressed series, and explicitly marking an episode watched also
restores it as evidence of renewed engagement.

`Hide watched movies` is a global discovery preference and defaults to on,
including for settings files created before the preference existed. Home and
every loaded catalog page filter against the same canonical watched-movie set.
Disabling it restores those cards immediately without changing library state or
requesting different data from catalog providers. Series and anime are not
removed by this movie-level preference.

## Invariants

- Watched updates are committed in one SQLite transaction.
- A series watched mutation changes exactly one selected episode.
- A title-level series action marks only the regular episodes present in its
  freshly loaded catalog snapshot.
- Unwatching an episode never clears watched state from sibling episodes.
- Marking a title watched never clears favorites or imported history.
- Repeating the same update is idempotent and does not duplicate episodes.
- New catalog episodes do not rewrite or invalidate earlier watched records.
- Continue Watching is based on exact episode identity, not only a numeric frontier.
- Continue Watching suppression is durable and independent of watched state.
- Hiding watched movies is presentation-only and never deletes catalog or
  library records.
- Season zero specials do not advance the regular-series watched frontier.
- Only an explicitly unwatched regular episode with at least one playable
  torrent enters Continue Watching.
- Missing display metadata is not fabricated. A history-only series appears
  after catalog metadata or imported metadata can identify it to the user.

## Tradeoffs

An older unmarked episode is intentionally considered unwatched even when a
higher episode is marked. This makes manual episode history deterministic and
avoids assuming that playback happened outside RedCrown.

Automatic preloading is deferred. It changes cache ownership, bandwidth use,
and user controls, so it requires a separate design rather than being coupled
to watched-state detection.

The context menu uses the browser popover top layer with native light-dismiss
and Escape behavior. Its actions remain ordinary buttons instead of claiming
the stricter ARIA menu keyboard contract.
