"use client";

import * as React from "react";
import dayjs from "dayjs";
import {
    Calendar as CalendarIcon,
    ChevronsLeftIcon,
    ChevronsRightIcon,
    XIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Calendar } from "@/components/ui/calendar";
import {
    Popover,
    PopoverContent,
    PopoverTrigger,
} from "@/components/ui/popover";
import { ScrollArea, ScrollBar } from "@/components/ui/scroll-area";

interface DateTimePickerProps {
    date: Date | string | null | undefined;
    setDate: (date: Date | undefined) => void;
    placeholder?: string;
    clearable?: boolean;
    disabled?: boolean;
    className?: string;
    i18n?: (key: string) => string;
}

export function DateTimePicker({
    date,
    setDate,
    placeholder,
    clearable = false,
    disabled = false,
    className,
    i18n,
}: DateTimePickerProps) {
    const parsed = React.useMemo(() => {
        if (!date) return undefined;
        const d = dayjs(date);
        return d.isValid() ? d : undefined;
    }, [date]);

    const validDate = parsed?.toDate();
    const minMonth = React.useMemo(() => new Date(2000, 0), []);
    const maxMonth = React.useMemo(
        () => new Date(new Date().getFullYear() + 20, 11),
        [],
    );

    const handleI18n = (key: string) => {
        if (i18n) {
            return i18n(key);
        }
        return key;
    };

    const handleDateChange = (newDate: Date | undefined) => {
        if (!newDate) {
            setDate(undefined);
            return;
        }
        const next = new Date(newDate);
        if (parsed) {
            next.setHours(parsed.hour(), parsed.minute(), parsed.second(), 0);
        }
        setDate(next);
    };

    const handleHourSelect = (hour: number) => {
        const base = parsed ?? dayjs().minute(0).second(0).millisecond(0);
        setDate(base.hour(hour).toDate());
    };

    const handleMinuteSelect = (minute: number) => {
        const base = parsed ?? dayjs().second(0).millisecond(0);
        setDate(base.minute(minute).toDate());
    };

    return (
        <div className={cn("flex items-center gap-1", className)}>
            <div className="min-w-0 flex-1">
                <Popover>
                    <PopoverTrigger asChild>
                        <Button
                            type="button"
                            variant="outline"
                            disabled={disabled}
                            className={cn(
                                "w-full justify-start text-left font-normal",
                                !parsed && "text-muted-foreground",
                            )}
                        >
                            <CalendarIcon className="mr-2 size-4 shrink-0" />
                            <span className="truncate">
                                {parsed
                                    ? parsed.format("YYYY-MM-DD HH:mm")
                                    : placeholder || handleI18n("pickDate")}
                            </span>
                        </Button>
                    </PopoverTrigger>
                    <PopoverContent className="w-auto p-0" align="start">
                        <div className="sm:flex">
                            <Calendar
                                mode="single"
                                captionLayout="dropdown"
                                startMonth={minMonth}
                                endMonth={maxMonth}
                                defaultMonth={validDate}
                                selected={validDate}
                                onSelect={handleDateChange}
                                autoFocus
                                footer={
                                    <div className="flex items-center gap-2">
                                        <Button
                                            type="button"
                                            variant="outline"
                                            size="sm"
                                            onClick={() => {
                                                const base = parsed ?? dayjs();
                                                setDate(
                                                    base
                                                        .subtract(1, "year")
                                                        .toDate(),
                                                );
                                            }}
                                        >
                                            <ChevronsLeftIcon className="h-4 w-4" />
                                        </Button>
                                        <Button
                                            type="button"
                                            variant="outline"
                                            size="sm"
                                            onClick={() => {
                                                const base = parsed ?? dayjs();
                                                setDate(
                                                    base
                                                        .add(1, "year")
                                                        .toDate(),
                                                );
                                            }}
                                        >
                                            <ChevronsRightIcon className="h-4 w-4" />
                                        </Button>
                                        <Button
                                            type="button"
                                            variant="outline"
                                            size="sm"
                                            onClick={() => {
                                                setDate(new Date());
                                            }}
                                        >
                                            {handleI18n("now")}
                                        </Button>
                                    </div>
                                }
                            />
                            <div className="flex flex-col divide-y sm:h-[305px] sm:flex-row sm:divide-x sm:divide-y-0">
                                <ScrollArea className="w-64 sm:w-auto">
                                    <div className="flex p-2 sm:flex-col">
                                        {Array.from(
                                            { length: 24 },
                                            (_, i) => i,
                                        ).map((hour) => (
                                            <Button
                                                key={hour}
                                                type="button"
                                                size="icon"
                                                variant={
                                                    parsed &&
                                                    parsed.hour() === hour
                                                        ? "secondary"
                                                        : "ghost"
                                                }
                                                className="aspect-square shrink-0 sm:w-full"
                                                onClick={() =>
                                                    handleHourSelect(hour)
                                                }
                                            >
                                                {hour
                                                    .toString()
                                                    .padStart(2, "0")}
                                            </Button>
                                        ))}
                                    </div>
                                    <ScrollBar
                                        orientation="horizontal"
                                        className="sm:hidden"
                                    />
                                </ScrollArea>
                                <ScrollArea className="w-64 sm:w-auto">
                                    <div className="flex p-2 sm:flex-col">
                                        {Array.from(
                                            { length: 12 },
                                            (_, i) => i * 5,
                                        ).map((minute) => (
                                            <Button
                                                key={minute}
                                                type="button"
                                                size="icon"
                                                variant={
                                                    parsed &&
                                                    parsed.minute() === minute
                                                        ? "secondary"
                                                        : "ghost"
                                                }
                                                className="aspect-square shrink-0 sm:w-full"
                                                onClick={() =>
                                                    handleMinuteSelect(minute)
                                                }
                                            >
                                                {minute
                                                    .toString()
                                                    .padStart(2, "0")}
                                            </Button>
                                        ))}
                                    </div>
                                    <ScrollBar
                                        orientation="horizontal"
                                        className="sm:hidden"
                                    />
                                </ScrollArea>
                            </div>
                        </div>
                    </PopoverContent>
                </Popover>
            </div>
            {clearable && parsed && !disabled && (
                <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-9 shrink-0"
                    onClick={() => setDate(undefined)}
                >
                    <XIcon className="size-4" />
                </Button>
            )}
        </div>
    );
}
