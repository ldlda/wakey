import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";
import path from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

const debugEnabled = (): boolean => {
  const explicit = process.env.DEBUG?.toLowerCase();
  if (explicit) {
    return explicit === "1" || explicit === "true" || explicit === "yes";
  }

  const rustLog = (process.env.RUST_LOG || "").toLowerCase();
  return rustLog.includes("debug") || rustLog.includes("trace");
};

const ReactCompilerConfig = {
  target: "18",
  runtimeModule: "react-compiler-runtime", // Redirects the missing specifier
};

export default defineConfig(({ command, mode, isPreview }) => {
  const env = loadEnv(mode, __dirname, "WAKEY_DEV_");
  const target = env.WAKEY_DEV_API_TARGET || "http://127.0.0.1:8080";
  const dev = command === "serve" && !isPreview;
  if (dev) {
    const url = new URL(target);
    if (
      !["http:", "https:"].includes(url.protocol) ||
      url.username ||
      url.password ||
      url.pathname !== "/" ||
      url.search ||
      url.hash
    ) {
      throw new Error(
        "WAKEY_DEV_API_TARGET must be an HTTP(S) origin without credentials or a path",
      );
    }
  }
  return {
    plugins: [
      react({
        babel: {
          plugins: [["babel-plugin-react-compiler", ReactCompilerConfig]],
        },
      }),
      tailwindcss(),
    ],
    base: "/ui/",
    resolve: {
      alias: {
        "@": path.resolve(__dirname, "src"),
      },
    },
    build: {
      sourcemap: debugEnabled(),
    },
    // Keep browser requests same-origin, including terminal/event WebSockets.
    // WAKEY_DEV_* is read only by this config, never exposed as VITE_* client data.
    server: dev
      ? {
          host: "127.0.0.1",
          proxy: {
            "^/api/": { target, changeOrigin: true, ws: true },
            "^/healthz$": { target, changeOrigin: true },
          },
        }
      : undefined,
  };
});
