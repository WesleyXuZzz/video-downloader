import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  build: {
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      output: {
        manualChunks(id) {
          const moduleId = id.replaceAll("\\", "/");

          if (
            moduleId.includes("/node_modules/react/")
            || moduleId.includes("/node_modules/react-dom/")
          ) {
            return "vendor-react";
          }

          if (
            moduleId.includes("/node_modules/@ant-design/icons/")
            || moduleId.includes("/node_modules/@ant-design/icons-svg/")
            || moduleId.includes("/node_modules/@ant-design/colors/")
            || moduleId.includes("/node_modules/@ant-design/fast-color/")
            || moduleId.includes("/node_modules/@rc-component/util/")
            || moduleId.includes("/node_modules/clsx/")
          ) {
            return "vendor-icons";
          }

          if (
            moduleId.includes("/node_modules/antd/")
            || moduleId.includes("/node_modules/@ant-design/")
          ) {
            return "vendor-antd";
          }

          if (moduleId.includes("/node_modules/@tauri-apps/")) {
            return "vendor-tauri";
          }
        },
      },
    },
  },
  clearScreen: false,
  server: {
    strictPort: true,
    host: "127.0.0.1",
    port: 1420,
  },
  envPrefix: ["VITE_", "TAURI_"],
});
