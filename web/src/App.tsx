import React from "react";
import { RouterProvider } from "react-router-dom";
import "@/app/globals.css";
import "@/index.css";
import router, { goToLogin } from "@/router.tsx";
import { ThemeProvider } from "@/components/theme-provider";
import { useAsync } from "react-async-hook";
import useUserStore from "./state/user";

export default function App() {
  const fetch = useUserStore((state) => state.fetch);
  useAsync(async () => {
    try {
      const isLogin = await fetch();
      if (!isLogin) {
        goToLogin();
      }
    } catch (err) {
      // TODO 出错提示
      console.error(err);
    }
  }, []);
  return (
    <React.StrictMode>
      <ThemeProvider storageKey="vite-ui-theme">
        <RouterProvider router={router} />
      </ThemeProvider>
    </React.StrictMode>
  );
}
