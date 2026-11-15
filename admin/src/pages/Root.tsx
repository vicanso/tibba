import { AppSidebar } from "@/components/app-sidebar";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import AppHeader from "@/components/app-header";
import { LOGIN } from "@/constants/route";
import { Outlet, useNavigate } from "react-router";
import { useEffect, useState } from "react";

export default function Root() {
    const key = "sidebarOpen";
    const sidebarOpen = window.localStorage.getItem(key);
    const [open, setOpen] = useState(sidebarOpen == "true" || !sidebarOpen);
    const navigate = useNavigate();

    useEffect(() => {
        const params = new URLSearchParams(window.location.search);
        const mfaRequired = params.get("mfa_required");
        const mfaToken = params.get("mfa_token");
        if (mfaRequired === "1" && mfaToken) {
            window.history.replaceState(
                {},
                "",
                window.location.pathname + window.location.hash,
            );
            navigate(
                `${LOGIN}?mfa_required=1&mfa_token=${encodeURIComponent(mfaToken)}`,
                { replace: true },
            );
        }
    }, [navigate]);

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
