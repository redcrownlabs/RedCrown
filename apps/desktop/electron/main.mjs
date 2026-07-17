import { app, BrowserWindow, ipcMain } from "electron";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { createInterface } from "node:readline";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const REQUEST_TIMEOUT_MS = 30_000;
let mainWindow;
let backend;
let backendReady;
let pending = new Map();

function backendPath() {
  if (process.env.REDCROWN_BACKEND_BIN) {
    return process.env.REDCROWN_BACKEND_BIN;
  }
  return join(root, "..", "..", "..", "backend", "target", "debug", "redcrown-desktop.exe");
}

function rejectPending(message) {
  for (const entry of pending.values()) {
    clearTimeout(entry.timer);
    entry.reject(new Error(message));
  }
  pending.clear();
}

function startBackend() {
  backend = spawn(backendPath(), [], {
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
  });
  backend.stderr.setEncoding("utf8");
  backend.stderr.on("data", (message) => console.error(String(message).trimEnd()));
  backend.on("error", (error) => rejectPending(`Backend failed to start: ${error.message}`));
  backend.on("exit", (code) => {
    rejectPending(`Backend stopped unexpectedly (exit ${String(code)})`);
    backend = undefined;
    backendReady = undefined;
    mainWindow?.webContents.send("redcrown:backend-status", "stopped");
  });

  createInterface({ input: backend.stdout }).on("line", (line) => {
    try {
      const response = JSON.parse(line);
      const entry = pending.get(response.id);
      if (!entry) return;
      pending.delete(response.id);
      clearTimeout(entry.timer);
      if (response.error) {
        entry.reject(new Error(response.error.message));
      } else {
        entry.resolve(response.result);
      }
    } catch (error) {
      console.error("Invalid backend response", error);
    }
  });
  backendReady = invokeBackend("health", {});
  return backendReady;
}

function invokeBackend(method, params) {
  if (!backend?.stdin.writable) {
    return Promise.reject(new Error("Backend is unavailable"));
  }
  const id = randomUUID();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Backend request timed out: ${method}`));
    }, REQUEST_TIMEOUT_MS);
    pending.set(id, { resolve, reject, timer });
    backend.stdin.write(`${JSON.stringify({ id, method, params })}\n`, (error) => {
      if (!error) return;
      clearTimeout(timer);
      pending.delete(id);
      reject(error);
    });
  });
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1440,
    height: 900,
    minWidth: 880,
    minHeight: 640,
    backgroundColor: "#0d1015",
    frame: false,
    autoHideMenuBar: true,
    show: false,
    webPreferences: {
      preload: join(root, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
    },
  });

  mainWindow.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  mainWindow.webContents.on("will-navigate", (event) => event.preventDefault());
  mainWindow.on("maximize", () => mainWindow?.webContents.send("redcrown:window-maximized", true));
  mainWindow.on("unmaximize", () => mainWindow?.webContents.send("redcrown:window-maximized", false));
  mainWindow.once("ready-to-show", () => mainWindow?.show());

  const developmentUrl = process.env.VITE_DEV_SERVER_URL ?? "http://127.0.0.1:5173";
  if (!app.isPackaged) {
    void mainWindow.loadURL(developmentUrl);
  } else {
    void mainWindow.loadFile(join(root, "..", "dist", "index.html"));
  }
}

ipcMain.handle("redcrown:invoke", async (_event, request) => {
  if (!request || typeof request.method !== "string" || typeof request.params !== "object") {
    throw new Error("Invalid renderer request");
  }
  if (!backend) await startBackend();
  else await backendReady;
  return invokeBackend(request.method, request.params);
});

ipcMain.handle("redcrown:window-control", (event, action) => {
  const window = BrowserWindow.fromWebContents(event.sender);
  if (!window || window !== mainWindow) {
    throw new Error("Window control request did not originate from the main window");
  }
  switch (action) {
    case "minimize":
      window.minimize();
      return window.isMaximized();
    case "toggle-maximize":
      if (window.isMaximized()) window.unmaximize();
      else window.maximize();
      return window.isMaximized();
    case "close":
      window.close();
      return false;
    case "is-maximized":
      return window.isMaximized();
    default:
      throw new Error("Unsupported window control action");
  }
});

app.whenReady().then(async () => {
  await startBackend();
  createWindow();
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});

app.on("before-quit", () => {
  rejectPending("Application is shutting down");
  backend?.stdin.end();
  setTimeout(() => backend?.kill(), 3_000).unref();
});
