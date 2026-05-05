"use client";

import React from "react";
import { Toaster } from "@/components/ui/sonner";
import { RouterProvider } from "react-router";
import { ThemeProvider } from "next-themes";
import { TooltipProvider } from "@/components/ui/tooltip";
import router from "@/routers";

export default function App() {
    return (
        <React.StrictMode>
            <ThemeProvider
                storageKey={"ui-theme"}
                attribute="class"
                enableSystem
            >
                <TooltipProvider>
                    <Toaster />
                    <RouterProvider router={router} />
                </TooltipProvider>
            </ThemeProvider>
        </React.StrictMode>
    );
}
