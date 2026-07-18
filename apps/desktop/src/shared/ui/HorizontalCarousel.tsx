import { useEffect, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent, ReactNode } from "react";
import { isCarouselDrag } from "./carousel-gesture";
import { Icon } from "./Icon";

export function HorizontalCarousel({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  const trackRef = useRef<HTMLUListElement>(null);
  const drag = useRef({ active: false, moved: false, startX: 0, startScroll: 0 });
  const [dragging, setDragging] = useState(false);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(true);

  useEffect(() => {
    const track = trackRef.current;
    if (!track) return;
    const update = () => {
      setCanScrollLeft(track.scrollLeft > 2);
      setCanScrollRight(track.scrollLeft + track.clientWidth < track.scrollWidth - 2);
    };
    update();
    track.addEventListener("scroll", update, { passive: true });
    const resizeObserver = new ResizeObserver(update);
    resizeObserver.observe(track);
    return () => {
      track.removeEventListener("scroll", update);
      resizeObserver.disconnect();
    };
  }, []);

  function scroll(direction: -1 | 1) {
    const track = trackRef.current;
    if (!track) return;
    track.scrollBy({ left: direction * track.clientWidth * 0.82, behavior: "smooth" });
  }

  function beginDrag(event: ReactPointerEvent<HTMLUListElement>) {
    if (event.button !== 0) return;
    drag.current = {
      active: true,
      moved: false,
      startX: event.clientX,
      startScroll: event.currentTarget.scrollLeft,
    };
  }

  function moveDrag(event: ReactPointerEvent<HTMLUListElement>) {
    if (!drag.current.active) return;
    const distance = event.clientX - drag.current.startX;
    if (!drag.current.moved && isCarouselDrag(distance)) {
      drag.current.moved = true;
      event.currentTarget.setPointerCapture(event.pointerId);
      setDragging(true);
    }
    if (!drag.current.moved) return;
    event.preventDefault();
    event.currentTarget.scrollLeft = drag.current.startScroll - distance;
  }

  function endDrag(event: ReactPointerEvent<HTMLUListElement>) {
    if (!drag.current.active) return;
    drag.current.active = false;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setDragging(false);
  }

  function suppressDraggedClick(event: ReactMouseEvent<HTMLUListElement>) {
    if (!drag.current.moved) return;
    event.preventDefault();
    event.stopPropagation();
    drag.current.moved = false;
  }

  return (
    <div className="carousel-shell">
      {canScrollLeft && (
        <button className="carousel-arrow carousel-arrow-left" onClick={() => scroll(-1)} aria-label={`Scroll ${label} left`}>
          <Icon name="left" />
        </button>
      )}
      <ul
        className={`landscape-track${dragging ? " dragging" : ""}`}
        role="list"
        ref={trackRef}
        onPointerDown={beginDrag}
        onPointerMove={moveDrag}
        onPointerUp={endDrag}
        onPointerCancel={(event) => {
          endDrag(event);
          drag.current.moved = false;
        }}
        onClickCapture={suppressDraggedClick}
      >
        {children}
      </ul>
      {canScrollRight && (
        <button className="carousel-arrow carousel-arrow-right" onClick={() => scroll(1)} aria-label={`Scroll ${label} right`}>
          <Icon name="right" />
        </button>
      )}
    </div>
  );
}

