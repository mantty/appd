import { defineConfig } from "astro/config";
import cloudflare from "@astrojs/cloudflare";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  output: "server",
  adapter: cloudflare(),
  // Set the dev server port to the default expected by appd.
  server: {
    port: 5173,
    strictPort: true,
  },
  vite: {
    plugins: [tailwindcss()],
  },
});
