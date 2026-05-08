import { AppSidebar } from "@/components/app-sidebar";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import AppHeader from "@/components/app-header";
import { Outlet } from "react-router";
import { useState } from "react";

export default function Root() {
    const key = "sidebarOpen";
    const sidebarOpen = window.localStorage.getItem(key);
    const [open, setOpen] = useState(sidebarOpen == "true" || !sidebarOpen);

    return (
        <SidebarProvider
            open={open}
            onOpenChange={(open) => {
                window.localStorage.setItem(key, open.toString());
                setOpen(open);
            }}
        >
            <AppSidebar />
            <SidebarInset className="w-[var(--sidebar-width)]">
                <AppHeader />
                <div className="flex flex-1 flex-col gap-4 p-4">
                    <Outlet />
                </div>
            </SidebarInset>
        </SidebarProvider>
    );
}
