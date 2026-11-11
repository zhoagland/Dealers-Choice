import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `npm run dev` proxies the API and WebSocket to a local dc-server (DEV_MODE=1 cargo run -p server).
const server = "http://127.0.0.1:8080";

export default defineConfig({
  plugins: [react()],
  // Relative asset URLs, so the app also works behind Discord's Activity proxy (web.md §13).
  base: "./",
  server: {
    proxy: {
      "/api": server,
      "/auth": server,
      "/healthz": server,
      "/ws": { target: server, ws: true },
    },
  },
});
