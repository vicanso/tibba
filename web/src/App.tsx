import React from "react";
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
  const fetch = useUserStore((state) => state.fetch);
  useAsync(async () => {
    try {
      const isLogin = await fetch();
      if (!isLogin) {
        goToLogin();
      }
    } catch (err) {
      toast({
        title: "Fetch user info fail",
        description: formatError(err),
      });
      console.error(err);
    }
  }, []);
  return (
    <React.StrictMode>
      <ThemeProvider storageKey="vite-ui-theme">
        <RouterProvider router={router} />
        <Toaster />
      </ThemeProvider>
    </React.StrictMode>
  );
}
