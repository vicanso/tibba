import path from "path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

function getModuleName(id: string) {
    const arr = id.split(path.sep);
    const index = arr.indexOf("node_modules");
    if (index === -1 || index === arr.length - 1) {
        return "";
    }
    return arr[index + 1];
}

function manualChunks(id: string) {
    const module = getModuleName(id);
    if (
        [
            "axios",
            "bytes",
            "crypto-js",
            "date-fns",
            "dayjs",
            "lodash-es",
            "nanoid",
            "react",
            "zod",
            "zustand",
        ].includes(module)
    ) {
        return "common";
    }
    if (id.includes("node_modules")) {
        return "vendor";
    }
    if (id.includes("components/ui")) {
        return "ui";
    }
}

// https://vite.dev/config/
export default defineConfig({
    plugins: [react(), tailwindcss()],
    resolve: {
        alias: {
            "@": path.resolve(__dirname, "./src"),
        },
    },
    base: "./",
    build: {
        chunkSizeWarningLimit: 1024 * 1024,
        rollupOptions: {
            output: {
                manualChunks,
            },
        },
    },
    server: {
        host: "0.0.0.0",
        proxy: {
            "/api": {
                target: "http://127.0.0.1:5000",
                rewrite: (path) => path.replace(/^\/api/, ""),
            },
        },
    },
});
