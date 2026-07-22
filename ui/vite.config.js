import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
	plugins: [svelte()],
	base: "/_/",
	build: {
		outDir: "../core/priv/static/admin",
		emptyOutDir: true,
	},
	server: {
		port: 5173,
		proxy: {
			"/api": "http://localhost:4000",
		},
	},
});
