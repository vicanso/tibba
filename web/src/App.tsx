import React, { useState } from "react";
import { RouterProvider } from "react-router-dom";
import "@/app/globals.css";
import "@/index.css";
import router, { goToLogin } from "@/router.tsx";
import { ThemeProvider } from "@/components/theme-provider";
import { useAsync } from "react-async-hook";
import { Toaster } from "@/components/ui/toaster";
import useUserStore from "@/state/user";
import { useToast } from "@/components/ui/use-toast";
import { formatError } from "@/helpers/util";

export default function App() {
  const { toast } = useToast();
  const [initialized, setInitialized] = useState(false);
  const fetch = useUserStore((state) => state.fetch);
  useAsync(async () => {
    try {
      const isLogin = await fetch();
      if (!isLogin) {
        goToLogin();
      }
    } catch (err) {
      toast({
        title: "获取用户信息失败",
        description: formatError(err),
      });
      console.error(err);
    } finally {
      setInitialized(true);
    }
  }, []);
  if (!initialized) {
    return <div className="text-center mt-5">正在初始化，请稍候...</div>;
  }
  return (
    <React.StrictMode>
      <ThemeProvider storageKey="vite-ui-theme">
        <RouterProvider router={router} />
        <Toaster />
      </ThemeProvider>
    </React.StrictMode>
  );
}
