import { useLayoutEffect, useRef, useState } from "react";

export type ContextAction = {
  id: string;
  label: string;
  disabled?: boolean;
  tone?: "default" | "danger";
  onSelect: () => void | Promise<void>;
};

export function ContextActionPopover({
  title,
  x,
  y,
  actions,
  onClose,
}: {
  title: string;
  x: number;
  y: number;
  actions: ContextAction[];
  onClose: () => void;
}) {
  const popoverRef = useRef<HTMLDivElement>(null);
  const [pending, setPending] = useState<string>();

  useLayoutEffect(() => {
    const popover = popoverRef.current;
    if (!popover) return;
    popover.showPopover();
    const bounds = popover.getBoundingClientRect();
    const gutter = 10;
    popover.style.left = `${Math.max(gutter, Math.min(x, window.innerWidth - bounds.width - gutter))}px`;
    popover.style.top = `${Math.max(gutter, Math.min(y, window.innerHeight - bounds.height - gutter))}px`;
    popover.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    return () => {
      if (popover.matches(":popover-open")) popover.hidePopover();
    };
  }, [x, y]);

  return (
    <div
      className="context-action-popover"
      popover="auto"
      ref={popoverRef}
      onToggle={(event) => {
        if (event.newState === "closed") onClose();
      }}
    >
      <p>{title}</p>
      <div className="context-action-list">
        {actions.map((action) => (
          <button
            className={action.tone === "danger" ? "danger" : undefined}
            disabled={pending != null || action.disabled}
            key={action.id}
            onClick={() => {
              setPending(action.id);
              void Promise.resolve(action.onSelect()).finally(onClose);
            }}
          >
            {pending === action.id ? "Working…" : action.label}
          </button>
        ))}
      </div>
    </div>
  );
}
