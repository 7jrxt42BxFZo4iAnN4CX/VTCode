import { defineConfig } from "vite";

const appInstance = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

export default defineConfig({
  base: "./",
  define: {
    __VTCODE_APP_INSTANCE__: JSON.stringify(appInstance),
  },
  server: {
    strictPort: true,
  },
});
