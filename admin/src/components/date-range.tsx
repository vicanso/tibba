"use client";

import * as React from "react";
import { CalendarIcon, XIcon } from "lucide-react";
import { DateRange } from "react-day-picker";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Calendar } from "@/components/ui/calendar";
import {
    Popover,
    PopoverContent,
    PopoverTrigger,
} from "@/components/ui/popover";
import { useI18n } from "@/i18n";
import dayjs from "dayjs";

export function DatePickerWithRange({
    className,
    onValueChange,
    defaultDateRange,
    placeholder,
}: {
    className?: string;
    onValueChange?: (value: string[]) => void;
    defaultDateRange?: string[];
    placeholder?: string;
}) {
    const componentI18n = useI18n("component");
    const [date, setDate] = React.useState<DateRange | undefined>(() => ({
        from: defaultDateRange?.[0]
            ? dayjs(defaultDateRange[0]).startOf("day").toDate()
            : undefined,
        to: defaultDateRange?.[1]
            ? dayjs(defaultDateRange[1]).endOf("day").toDate()
            : undefined,
    }));

    const minMonth = React.useMemo(() => new Date(2000, 0), []);
    const maxMonth = React.useMemo(
        () => new Date(new Date().getFullYear() + 20, 11),
        [],
    );

    const formatDateRange = (range: DateRange) => {
        if (!range.from || !range.to) {
            return [];
        }

        return [
            dayjs(range.from).format("YYYY-MM-DD"),
            dayjs(range.to).format("YYYY-MM-DD"),
        ];
    };

    return (
        <div className={cn("flex items-center gap-1", className)}>
            <div className="min-w-0 flex-1">
                <Popover>
                    <PopoverTrigger asChild>
                        <Button
                            id="date"
                            type="button"
                            variant="outline"
                            className={cn(
                                "w-full min-w-[240px] justify-start text-left font-normal",
                                !date?.from && "text-muted-foreground",
                            )}
                        >
                            <CalendarIcon className="mr-2 size-4 shrink-0" />
                            <span className="truncate">
                                {date?.from && date?.to
                                    ? formatDateRange(date).join(" - ")
                                    : placeholder || componentI18n("pickDate")}
                            </span>
                        </Button>
                    </PopoverTrigger>
                    <PopoverContent className="w-auto p-0" align="start">
                        <Calendar
                            captionLayout="dropdown"
                            startMonth={minMonth}
                            endMonth={maxMonth}
                            mode="range"
                            defaultMonth={date?.from}
                            selected={date}
                            onSelect={(value) => {
                                setDate(value);
                                if (onValueChange) {
                                    if (value && value.from && value.to) {
                                        onValueChange([
                                            dayjs(value.from)
                                                .startOf("day")
                                                .toISOString(),
                                            dayjs(value.to)
                                                .endOf("day")
                                                .toISOString(),
                                        ]);
                                    } else {
                                        onValueChange([]);
                                    }
                                }
                            }}
                            numberOfMonths={2}
                        />
                    </PopoverContent>
                </Popover>
            </div>
            {(date?.from || date?.to) && (
                <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-9 shrink-0"
                    onClick={() => {
                        setDate(undefined);
                        onValueChange?.([]);
                    }}
                >
                    <XIcon className="size-4" />
                </Button>
            )}
        </div>
    );
}
