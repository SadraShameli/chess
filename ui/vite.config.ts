import { defineConfig } from "vite";
import type { Plugin } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

const nativeTest: Plugin = {
  name: "native-test-entry",
  apply: (_configuration, environment) => environment.mode === "native-test",
  transformIndexHtml: {
    order: "pre",
    handler: () => [
      {
        tag: "script",
        attrs: { type: "module", src: "/src/desktop/native_test.ts" },
        injectTo: "head",
      },
    ],
  },
};

export default defineConfig({
  plugins: [vue(), tailwindcss(), nativeTest],
  resolve: { tsconfigPaths: true },
  clearScreen: false,
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
  build: { target: "es2023", sourcemap: true },
});
