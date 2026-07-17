const { contextBridge, ipcRenderer } = require("electron");

contextBridge.exposeInMainWorld("redcrown", {
  invoke(method, params = {}) {
    return ipcRenderer.invoke("redcrown:invoke", { method, params });
  },
  windowControls: {
    minimize() {
      return ipcRenderer.invoke("redcrown:window-control", "minimize");
    },
    toggleMaximize() {
      return ipcRenderer.invoke("redcrown:window-control", "toggle-maximize");
    },
    close() {
      return ipcRenderer.invoke("redcrown:window-control", "close");
    },
    isMaximized() {
      return ipcRenderer.invoke("redcrown:window-control", "is-maximized");
    },
    onMaximized(callback) {
      const listener = (_event, maximized) => callback(maximized);
      ipcRenderer.on("redcrown:window-maximized", listener);
      return () => ipcRenderer.removeListener("redcrown:window-maximized", listener);
    },
  },
});
