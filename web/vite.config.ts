import path from "path"
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react-swc'

function manualChunks(id) {
  if (id.includes("node_modules")) {
    if (id.includes("@radix-ui")) {
      return "radix";
    }
    if (id.includes("react")) {
      return "reacts";
    }
    return "vendor";
  }
  if (id.includes("/components/")) {
    return "components";
  }
  if (id.includes("/helpers/")) {
    return "helpers";
  }
}

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks,
      }
    }
  },
  server: {
    proxy: {
      "/api": {
        target: "http://127.0.0.1:5000",
      },
    },
  }
})
