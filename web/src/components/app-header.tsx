"use client";

import {
    Breadcrumb,
    BreadcrumbItem,
    BreadcrumbList,
    BreadcrumbPage,
    BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { Separator } from "@/components/ui/separator";
import { SidebarTrigger } from "@/components/ui/sidebar";
import useBasicState from "@/states/basic";
import { Link } from "react-router";
import { useShallow } from "zustand/react/shallow";

export default function AppHeader() {
    const [pageHeaderNavigation] = useBasicState(
        useShallow((state) => [state.pageHeaderNavigation]),
    );

    const renderBreadcrumb = () => {
        const count = pageHeaderNavigation.length;
        const items: React.ReactNode[] = [];
        pageHeaderNavigation.forEach((item, index) => {
            const key = `${item.url}-${index}-${item.title}`;
            if (index === count - 1) {
                items.push(
                    <BreadcrumbItem key={key}>
                        <BreadcrumbPage className="flex items-center gap-2">
                            {item.icon}
                            {item.title}
                        </BreadcrumbPage>
                    </BreadcrumbItem>,
                );
                return;
            }
            if (item.url) {
                items.push(
                    <BreadcrumbItem
                        key={key}
                        className="flex items-center gap-2"
                    >
                        <Link to={item.url}>
                            {item.icon}
                            {item.title}
                        </Link>
                    </BreadcrumbItem>,
                );
            }
            if (index !== count - 1) {
                items.push(
                    <BreadcrumbSeparator
                        className="hidden md:block"
                        key={key + "separator"}
                    />,
                );
            }
        });
        return items;
    };

    return (
        <header className="flex h-16 shrink-0 items-center gap-2 border-b px-4">
            <SidebarTrigger className="-ml-1 cursor-pointer" />
            <Separator orientation="vertical" className="mr-2 h-4" />
            <Breadcrumb>
                <BreadcrumbList>{renderBreadcrumb()}</BreadcrumbList>
            </Breadcrumb>
        </header>
    );
}
