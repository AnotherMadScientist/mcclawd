import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 8080,
    proxy: {
      "/api": {
        target: "http://localhost:9090",
        changeOrigin: true,
        ws: true,
        timeout: 30000,
        configure: (proxy) => {
          proxy.on("error", (err, _req, res) => {
            console.warn("[vite proxy] Backend error:", err.message);
            if (res && "writeHead" in res && !res.headersSent) {
              (res as import("http").ServerResponse).writeHead(503, {
                "Content-Type": "application/json",
              });
              (res as import("http").ServerResponse).end(
                JSON.stringify({ error: "Backend unavailable" }),
              );
            }
          });
        },
      },
    },
  },
});
