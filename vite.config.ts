import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 会加载固定的开发地址，端口被占用时直接失败而不是静默换端口。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
