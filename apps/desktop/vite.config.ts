import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  // Packaged Electron renders this build from file://. Root-relative URLs would
  // resolve against the drive root instead of the adjacent dist directory.
  base: "./",
  plugins: [react(), tailwindcss()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
  },
  build: {
    target: "chrome142",
    sourcemap: true,
  },
});
