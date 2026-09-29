import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Release 빌드는 dist/ 를 Tauri EXE 안에 내장한다. 개발 서버는 개발 중에만 쓴다.
export default defineConfig(() => ({
  plugins: [react()],
  clearScreen: false,
  base: "./",
  server: {
    port: 5173,
    strictPort: true,
    host: "127.0.0.1",
  },
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    chunkSizeWarningLimit: 1500,
  },
}));
