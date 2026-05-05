"use client";
import {
    Command,
    CommandEmpty,
    CommandGroup,
    CommandInput,
    CommandItem,
} from "@/components/ui/command";
import {
    Popover,
    PopoverContent,
    PopoverTrigger,
} from "@/components/ui/popover";
import { Badge } from "@/components/ui/badge";
import { X, Check } from "lucide-react";
import React from "react";
import { cn } from "@/lib/utils";

interface Option {
    value: string;
    label: string;
}

export function MultiSelect({
    options,
    selected,
    onChange,
    disabled,
}: {
    options: Option[];
    selected: string[];
    disabled?: boolean;
    onChange: (values: string[]) => void;
}) {
    const [open, setOpen] = React.useState(false);

    return (
        <Popover open={disabled ? false : open} onOpenChange={setOpen}>
            <PopoverTrigger asChild>
                <div className="border rounded-md p-2 flex flex-wrap gap-1 cursor-pointer">
                    {selected.map((value) => (
                        <Badge key={value} variant="secondary">
                            {options.find((opt) => opt.value === value)?.label}
                            <button
                                disabled={disabled}
                                className="ml-1"
                                onClick={(e) => {
                                    e.stopPropagation();
                                    onChange(
                                        selected.filter((v) => v !== value),
                                    );
                                }}
                            >
                                <X size={14} />
                            </button>
                        </Badge>
                    ))}
                    <button
                        className="text-sm text-muted-foreground"
                        onClick={(e) => {
                            setOpen(true);
                            e.preventDefault();
                        }}
                    >
                        Select items...
                    </button>
                </div>
            </PopoverTrigger>
            <PopoverContent className="w-[200px] p-0">
                <Command>
                    <CommandInput placeholder="Search..." />
                    <CommandEmpty>No option found.</CommandEmpty>
                    <CommandGroup>
                        {options.map((option) => (
                            <CommandItem
                                key={option.value}
                                onSelect={() => {
                                    onChange(
                                        selected.includes(option.value)
                                            ? selected.filter(
                                                  (v) => v !== option.value,
                                              )
                                            : [...selected, option.value],
                                    );
                                }}
                            >
                                <div
                                    className={cn(
                                        "mr-2 flex h-4 w-4 items-center justify-center rounded-sm border",
                                        selected.includes(option.value)
                                            ? "bg-primary border-primary"
                                            : "opacity-50",
                                    )}
                                >
                                    {selected.includes(option.value) && (
                                        <Check className="h-3 w-3" />
                                    )}
                                </div>
                                {option.label}
                            </CommandItem>
                        ))}
                    </CommandGroup>
                </Command>
            </PopoverContent>
        </Popover>
    );
}
