/** Prevents ordinary click jitter from activating carousel dragging. */
const DRAG_ACTIVATION_DISTANCE_PX = 6;

export function isCarouselDrag(horizontalDistance: number) {
  return Math.abs(horizontalDistance) > DRAG_ACTIVATION_DISTANCE_PX;
}
