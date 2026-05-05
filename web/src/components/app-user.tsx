"use client";
import { cn } from "@/lib/utils";
import {
    BellIcon,
    CreditCardIcon,
    LogOutIcon,
    MoreVerticalIcon,
    UserCircleIcon,
    Loader2Icon,
    LogInIcon,
    CheckIcon,
    SunIcon,
    MoonIcon,
    SunMoonIcon,
    LanguagesIcon,
} from "lucide-react";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuGroup,
    DropdownMenuItem,
    DropdownMenuLabel,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Button } from "@/components/ui/button";
import {
    SidebarMenu,
    SidebarMenuButton,
    SidebarMenuItem,
    useSidebar,
} from "@/components/ui/sidebar";
import useUserState from "@/states/user";
import { useShallow } from "zustand/react/shallow";
import { useI18n } from "@/i18n";
import { toast } from "sonner";
import { formatError } from "@/helpers/util";
import { Link } from "react-router";
import { LOGIN, PROFILE } from "@/constants/route";
import { useEffect, useState, useCallback } from "react";
import { useTheme } from "next-themes";
import i18n from "@/i18n";
import dayjs from "dayjs";

export function AppUser() {
    const [processing, setProcessing] = useState(false);
    const { setTheme, theme } = useTheme();
    const lang = i18n.language;
    const { open } = useSidebar();

    const commonI18n = useI18n("common");
    const appUserI18n = useI18n("appUser");
    const [initialized, fetch, userInfo, logout, refresh] = useUserState(
        useShallow((state) => [
            state.initialized,
            state.fetch,
            state.data,
            state.logout,
            state.refresh,
        ]),
    );

    const handleLogout = async () => {
        if (processing) {
            return;
        }
        setProcessing(true);
        try {
            await logout();
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setProcessing(false);
        }
    };

    const handleFetch = useCallback(async () => {
        try {
            const user = await fetch();
            if (user.account && user.can_renew) {
                const expiredAt = dayjs(user.expired_at);
                const offset = 24 * 3600 * 1000;
                if (expiredAt.valueOf() - Date.now() < offset) {
                    // refresh session
                    refresh().catch((err) => {
                        console.error(err);
                    });
                }
            }
        } catch (err) {
            toast.error(formatError(err));
        }
    }, [fetch, refresh]);

    useEffect(() => {
        handleFetch();
    }, [handleFetch]);

    const iconClassName = "mr-2 h-4 w-4";

    const renderUser = (className: string) => {
        return (
            <>
                <Avatar className={cn("h-8 w-8 rounded-lg", className)}>
                    <AvatarImage src={userInfo.avatar} alt={userInfo.account} />
                    <AvatarFallback className="rounded-lg">
                        {userInfo.account.slice(0, 2).toUpperCase()}
                    </AvatarFallback>
                </Avatar>
                <div className="grid flex-1 text-left text-sm leading-tight">
                    <span className="truncate font-medium">
                        {processing ? (
                            <Loader2Icon className="size-4 animate-spin" />
                        ) : (
                            userInfo.account
                        )}
                    </span>
                    <span className="truncate text-xs text-muted-foreground">
                        {userInfo.email}
                    </span>
                </div>
            </>
        );
    };

    const renderTheme = () => {
        return (
            <>
                <DropdownMenuItem
                    className="cursor-pointer"
                    onClick={() => {
                        setTheme("system");
                    }}
                >
                    {theme == "system" && (
                        <CheckIcon className={iconClassName} />
                    )}
                    {theme != "system" && (
                        <SunMoonIcon className={iconClassName} />
                    )}
                    <span>{appUserI18n("themeSystem")}</span>
                </DropdownMenuItem>
                <DropdownMenuItem
                    className="cursor-pointer"
                    onClick={() => {
                        setTheme("dark");
                    }}
                >
                    {theme == "dark" && <CheckIcon className={iconClassName} />}
                    {theme != "dark" && <MoonIcon className={iconClassName} />}
                    <span>{appUserI18n("themeDark")}</span>
                </DropdownMenuItem>
                <DropdownMenuItem
                    className="cursor-pointer"
                    onClick={() => {
                        setTheme("light");
                    }}
                >
                    {theme == "light" && (
                        <CheckIcon className={iconClassName} />
                    )}
                    {theme != "light" && <SunIcon className={iconClassName} />}
                    <span>{appUserI18n("themeLight")}</span>
                </DropdownMenuItem>
            </>
        );
    };

    const renderLanguage = () => {
        const zhLang = "zh";
        const enLang = "en";
        return (
            <>
                <DropdownMenuItem
                    className="cursor-pointer"
                    onClick={() => {
                        i18n.changeLanguage(zhLang);
                    }}
                >
                    {lang == zhLang && <CheckIcon className={iconClassName} />}
                    {lang != zhLang && (
                        <LanguagesIcon className={iconClassName} />
                    )}
                    中文
                </DropdownMenuItem>
                <DropdownMenuItem
                    className="cursor-pointer"
                    onClick={() => {
                        i18n.changeLanguage(enLang);
                    }}
                >
                    {lang == enLang && <CheckIcon className={iconClassName} />}
                    {lang != enLang && (
                        <LanguagesIcon className={iconClassName} />
                    )}
                    English
                </DropdownMenuItem>
            </>
        );
    };

    const { isMobile } = useSidebar();
    let container = <></>;
    if (!initialized) {
        container = (
            <div className="flex items-center justify-center">
                <Loader2Icon className="size-4 animate-spin" />
                <span className="ml-2">{commonI18n("loading")}</span>
            </div>
        );
    } else if (!userInfo.account) {
        container = (
            <Button
                variant={open ? "outline" : "ghost"}
                size={open ? "lg" : "icon"}
                className="w-full"
            >
                <Link
                    to={LOGIN}
                    className="w-full h-full flex items-center justify-center gap-2"
                >
                    <LogInIcon />
                    {open && appUserI18n("login")}
                </Link>
            </Button>
        );
    } else {
        container = (
            <DropdownMenu>
                <DropdownMenuTrigger asChild>
                    <SidebarMenuButton
                        size="lg"
                        className="data-[state=open]:bg-sidebar-accent data-[state=open]:text-sidebar-accent-foreground cursor-pointer"
                    >
                        {renderUser("grayscale")}
                        <MoreVerticalIcon className="ml-auto size-4" />
                    </SidebarMenuButton>
                </DropdownMenuTrigger>
                <DropdownMenuContent
                    className="w-[--radix-dropdown-menu-trigger-width] min-w-56 rounded-lg"
                    side={isMobile ? "bottom" : "right"}
                    align="end"
                    sideOffset={4}
                >
                    <DropdownMenuLabel className="p-0 font-normal">
                        <div className="flex items-center gap-2 px-1 py-1.5 text-left text-sm">
                            {renderUser("")}
                        </div>
                    </DropdownMenuLabel>
                    <DropdownMenuSeparator />
                    <DropdownMenuGroup>
                        <DropdownMenuItem asChild>
                            <Link to={PROFILE} className="cursor-pointer">
                                <UserCircleIcon />
                                {appUserI18n("profile")}
                            </Link>
                        </DropdownMenuItem>
                        <DropdownMenuItem disabled>
                            <CreditCardIcon />
                            Billing
                        </DropdownMenuItem>
                        <DropdownMenuItem disabled>
                            <BellIcon />
                            Notifications
                        </DropdownMenuItem>
                        <DropdownMenuSeparator />
                        {renderTheme()}
                        <DropdownMenuSeparator />
                        {renderLanguage()}
                    </DropdownMenuGroup>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem
                        onClick={handleLogout}
                        className="cursor-pointer"
                    >
                        <LogOutIcon />
                        {appUserI18n("logout")}
                    </DropdownMenuItem>
                </DropdownMenuContent>
            </DropdownMenu>
        );
    }
    return (
        <SidebarMenu>
            <SidebarMenuItem>{container}</SidebarMenuItem>
        </SidebarMenu>
    );
}
