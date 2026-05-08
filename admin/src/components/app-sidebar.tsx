"use client";

import * as React from "react";
import { AppSwitcher } from "@/components/app-switcher";
import {
    Sidebar,
    SidebarContent,
    SidebarGroup,
    SidebarGroupContent,
    SidebarGroupLabel,
    SidebarHeader,
    SidebarMenu,
    SidebarMenuButton,
    SidebarMenuItem,
    SidebarRail,
    SidebarFooter,
} from "@/components/ui/sidebar";
import { AppUser } from "@/components/app-user";
import { useShallow } from "zustand/react/shallow";
import { useI18n } from "@/i18n";
import { Link } from "react-router";
import { useLocation } from "react-router";
import useUserState from "@/states/user";
import useBasicState from "@/states/basic";
import { useEffect } from "react";
export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
    const location = useLocation();
    const [selectedApp] = useBasicState(
        useShallow((state) => [state.selectedApp]),
    );
    const [user] = useUserState(useShallow((state) => [state.data]));
    const [mainNav, updateMainNav, resetMainNav] = useUserState(
        useShallow((state) => [
            state.mainNav,
            state.updateMainNav,
            state.resetMainNav,
        ]),
    );

    useEffect(() => {
        if (user.account) {
            updateMainNav(selectedApp, user.roles || []);
        } else {
            resetMainNav();
        }
    }, [selectedApp, updateMainNav, user.account, user.roles, resetMainNav]);

    const sidebarI18n = useI18n("sidebar");
    const isActive = (url: string) => {
        const current = url.split("?")[0];
        return current === location.pathname;
    };
    return (
        <Sidebar collapsible="icon" {...props}>
            <SidebarHeader>
                <AppSwitcher />
            </SidebarHeader>
            <SidebarContent>
                {mainNav.map((item) => (
                    <SidebarGroup key={item.title}>
                        <SidebarGroupLabel>
                            {sidebarI18n(item.title)}
                        </SidebarGroupLabel>
                        <SidebarGroupContent>
                            <SidebarMenu>
                                {item.items.map((item) => (
                                    <SidebarMenuItem
                                        key={item.title}
                                        className="text-nowrap"
                                        title={sidebarI18n(item.title)}
                                    >
                                        <SidebarMenuButton
                                            asChild
                                            isActive={isActive(item.url)}
                                        >
                                            <Link to={item.url}>
                                                {<item.icon className="mr-1" />}
                                                {sidebarI18n(item.title)}
                                            </Link>
                                        </SidebarMenuButton>
                                    </SidebarMenuItem>
                                ))}
                            </SidebarMenu>
                        </SidebarGroupContent>
                    </SidebarGroup>
                ))}
            </SidebarContent>
            <SidebarRail />
            <SidebarFooter>
                <AppUser />
            </SidebarFooter>
        </Sidebar>
    );
}
