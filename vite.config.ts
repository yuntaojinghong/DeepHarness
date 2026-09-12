import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  build: {
    // `frontendDist` 指向 `dist/`，Tauri 会把该目录**整体**嵌入二进制。
    // 若关闭清空（emptyOutDir: false），历史哈希 bundle 会逐次堆积并全被
    // 打包进安装包 —— 实测堆到 13.4MB 死重量，且会把旧品牌字符串一起发出去。
    // 因此这里必须保持清空，任何需要跨构建保留的东西都不应放进 dist/。
    emptyOutDir: true,
    rollupOptions: {
      output: {
        // 把体积大且极少变动的 React 运行时单独成块：
        // 业务代码改动不会让它失效，块也更便于定位体积来源。
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (/[\\/]node_modules[\\/](react|react-dom|scheduler|zustand)[\\/]/.test(id)) {
            return "vendor-react";
          }
          if (id.includes("@tauri-apps")) return "vendor-tauri";
          return undefined;
        },
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
