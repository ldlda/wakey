import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function sourcemapEnabled(): boolean {
  const explicit = process.env.UI_SOURCEMAP?.toLowerCase();
  if (explicit) {
    return explicit === "1" || explicit === "true" || explicit === "yes";
  }

  const rustLog = (process.env.RUST_LOG || "").toLowerCase();
  return rustLog.includes("debug") || rustLog.includes("trace");
}

export default defineConfig({
  plugins: [
    react({
      babel: {
        plugins: ["babel-plugin-react-compiler"],
      },
    }),
  ],
  base: "/ui/",
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  build: {
    sourcemap: sourcemapEnabled(),
  },
});
