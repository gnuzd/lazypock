import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  base: "/_/",
  build: {
    outDir: "../core/priv/static/admin",
    emptyOutDir: true,
    modulePreload: false,
  },
  server: {
    port: 5173,
    proxy: {
      "/api": "http://localhost:4000",
    },
  },
  optimizeDeps: {
    exclude: ["@lucide/svelte"],
  },
  ssr: {
    noExternal: ["@lucide/svelte"],
  },
});
