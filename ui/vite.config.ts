import { defineConfig } from "vite";
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

export default defineConfig({
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
});
