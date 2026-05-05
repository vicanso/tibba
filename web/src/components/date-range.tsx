"use client";

import * as React from "react";
import { CalendarIcon } from "lucide-react";
import { DateRange } from "react-day-picker";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Calendar } from "@/components/ui/calendar";
import {
    Popover,
    PopoverContent,
    PopoverTrigger,
} from "@/components/ui/popover";
import dayjs from "dayjs";

export function DatePickerWithRange({
    className,
    onValueChange,
    defaultDateRange,
}: {
    className?: string;
    onValueChange?: (value: string[]) => void;
    defaultDateRange?: string[];
}) {
    const [date, setDate] = React.useState<DateRange | undefined>({
        from: defaultDateRange?.[0]
            ? dayjs(defaultDateRange[0]).startOf("day").toDate()
            : undefined,
        to: defaultDateRange?.[1]
            ? dayjs(defaultDateRange[1]).endOf("day").toDate()
            : undefined,
    });

    const formatDateRange = (date: DateRange) => {
        if (!date.from || !date.to) {
            return [];
        }

        return [
            dayjs(date.from).format("YYYY-MM-DD"),
            dayjs(date.to).format("YYYY-MM-DD"),
        ];
    };

    return (
        <div className={cn("grid gap-2", className)}>
            <Popover>
                <PopoverTrigger asChild>
                    <Button
                        id="date"
                        variant={"outline"}
                        className={cn(
                            "w-[300px] justify-start text-left font-normal",
                            !date && "text-muted-foreground",
                        )}
                    >
                        <CalendarIcon />
                        {date?.from && date?.to ? (
                            formatDateRange(date).join(" - ")
                        ) : (
                            <span>Pick a date</span>
                        )}
                    </Button>
                </PopoverTrigger>
                <PopoverContent className="w-auto p-0" align="start">
                    <Calendar
                        captionLayout="dropdown"
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
    );
}
