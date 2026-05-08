"use client";

import * as React from "react";
import dayjs from "dayjs";
import {
    Calendar as CalendarIcon,
    ChevronsLeftIcon,
    ChevronsRightIcon,
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
import { useCallback } from "react";
interface DateTimePickerProps {
    date: Date | undefined;
    setDate: (date: Date | undefined) => void;
    className?: string;
    i18n?: (key: string) => string;
}

export function DateTimePicker({
    date,
    setDate,
    className,
    i18n,
}: DateTimePickerProps) {
    const currentDate = dayjs(date);
    // 使用 ref 来跟踪是否是用户手动更改时间
    const isUserTimeChange = React.useRef(false);

    const handleI18n = (key: string) => {
        if (i18n) {
            return i18n(key);
        }
        return key;
    };

    // 从日期中提取时间
    const getTimeFromDate = useCallback(
        (date: Date | undefined) => {
            if (!date) return "";
            return currentDate.format("HH:mm");
        },
        [currentDate],
    );

    // 初始时间状态
    const [selectedTime, setSelectedTime] = React.useState<string>(
        getTimeFromDate(date),
    );

    // 处理日期变化
    const handleDateChange = (newDate: Date | undefined) => {
        if (newDate) {
            // 如果已有时间，保留时间部分
            if (selectedTime) {
                const [hours, minutes] = selectedTime.split(":").map(Number);
                const dateWithTime = new Date(newDate);
                dateWithTime.setHours(hours);
                dateWithTime.setMinutes(minutes);
                setDate(dateWithTime);
            } else {
                setDate(newDate);
            }
        } else {
            setDate(undefined);
        }
    };

    // 处理时间变化
    const handleTimeChange = (hours: string, minutes: string) => {
        isUserTimeChange.current = true;
        const newTime = `${hours}:${minutes}`;
        setSelectedTime(newTime);

        if (date) {
            const newDate = new Date(date);
            newDate.setHours(parseInt(hours, 10));
            newDate.setMinutes(parseInt(minutes, 10));
            setDate(newDate);
        }
    };

    // 当外部日期变化时更新时间（仅当不是用户手动更改时）
    React.useEffect(() => {
        if (!isUserTimeChange.current && date) {
            setSelectedTime(getTimeFromDate(date));
        }
        isUserTimeChange.current = false;
    }, [date, getTimeFromDate]);

    return (
        <div className={cn("flex items-center gap-2", className)}>
            <div className="flex-1">
                <Popover>
                    <PopoverTrigger asChild>
                        <Button
                            variant={"outline"}
                            className={cn(
                                "w-full justify-start text-left font-normal",
                                !date && "text-muted-foreground",
                            )}
                        >
                            <CalendarIcon className="mr-2 h-4 w-4" />
                            {date ? (
                                currentDate.format("YYYY-MM-DD HH:mm")
                            ) : (
                                <span>{handleI18n("pickDate")}</span>
                            )}
                        </Button>
                    </PopoverTrigger>
                    <PopoverContent className="w-auto p-0" align="start">
                        <div className="sm:flex">
                            <Calendar
                                mode="single"
                                selected={date}
                                onSelect={handleDateChange}
                                initialFocus
                                footer={
                                    <div className="flex items-center gap-2">
                                        <Button
                                            variant="outline"
                                            size="sm"
                                            onClick={() => {
                                                let selectedDate = new Date();
                                                if (currentDate.isValid()) {
                                                    selectedDate = new Date(
                                                        currentDate.valueOf(),
                                                    );
                                                }
                                                selectedDate.setFullYear(
                                                    selectedDate.getFullYear() -
                                                        1,
                                                );
                                                setDate(selectedDate);
                                            }}
                                        >
                                            <ChevronsLeftIcon className="h-4 w-4" />
                                        </Button>
                                        <Button
                                            variant="outline"
                                            size="sm"
                                            onClick={() => {
                                                let selectedDate = new Date();
                                                if (currentDate.isValid()) {
                                                    selectedDate = new Date(
                                                        currentDate.valueOf(),
                                                    );
                                                }
                                                selectedDate.setFullYear(
                                                    selectedDate.getFullYear() +
                                                        1,
                                                );
                                                setDate(selectedDate);
                                            }}
                                        >
                                            <ChevronsRightIcon className="h-4 w-4" />
                                        </Button>
                                        <Button
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
                            <div className="flex flex-col sm:flex-row sm:h-[305px] divide-y sm:divide-y-0 sm:divide-x">
                                <ScrollArea className="w-64 sm:w-auto">
                                    <div className="flex sm:flex-col p-2">
                                        {Array.from({ length: 24 }, (_, i) => i)
                                            .reverse()
                                            .map((hour) => (
                                                <Button
                                                    key={hour}
                                                    size="icon"
                                                    variant={
                                                        date &&
                                                        currentDate.hour() ===
                                                            hour
                                                            ? "secondary"
                                                            : "ghost"
                                                    }
                                                    className="sm:w-full shrink-0 aspect-square"
                                                    onClick={() =>
                                                        handleTimeChange(
                                                            hour.toString(),
                                                            currentDate
                                                                .minute()
                                                                .toString(),
                                                        )
                                                    }
                                                >
                                                    {hour}
                                                </Button>
                                            ))}
                                    </div>
                                    <ScrollBar
                                        orientation="horizontal"
                                        className="sm:hidden"
                                    />
                                </ScrollArea>
                                <ScrollArea className="w-64 sm:w-auto">
                                    <div className="flex sm:flex-col p-2">
                                        {Array.from(
                                            { length: 12 },
                                            (_, i) => i * 5,
                                        ).map((minute) => (
                                            <Button
                                                key={minute}
                                                size="icon"
                                                variant={
                                                    date &&
                                                    currentDate.minute() ===
                                                        minute
                                                        ? "secondary"
                                                        : "ghost"
                                                }
                                                className="sm:w-full shrink-0 aspect-square"
                                                onClick={() =>
                                                    handleTimeChange(
                                                        currentDate
                                                            .hour()
                                                            .toString(),
                                                        minute.toString(),
                                                    )
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
        </div>
    );
}
