import { useEffect, useState } from "react";
import { Icon } from "./Icon";

export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let active = true;
    void window.redcrown.windowControls.isMaximized().then((state) => {
      if (active) setMaximized(state);
    });
    const unsubscribe = window.redcrown.windowControls.onMaximized(setMaximized);
    return () => {
      active = false;
      unsubscribe();
    };
  }, []);

  return (
    <div className="window-controls" aria-label="Window controls">
      <button onClick={() => void window.redcrown.windowControls.minimize()} aria-label="Minimize window">
        <Icon name="minimize" />
      </button>
      <button
        onClick={() => void window.redcrown.windowControls.toggleMaximize().then(setMaximized)}
        aria-label={maximized ? "Restore window" : "Maximize window"}
      >
        <Icon name={maximized ? "restore" : "maximize"} />
      </button>
      <button className="window-close" onClick={() => void window.redcrown.windowControls.close()} aria-label="Close window">
        <Icon name="close" />
      </button>
    </div>
  );
}

