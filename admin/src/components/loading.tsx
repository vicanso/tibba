"use client";
import { useI18n } from "@/i18n";
import { Loader2Icon } from "lucide-react";

export function Loading() {
    const commonI18n = useI18n("common");
    return (
        <div className="flex items-center justify-center">
            <Loader2Icon className="size-4 animate-spin" />
            <span className="ml-2">{commonI18n("loading")}</span>
        </div>
    );
}
