"use client";
import { cn } from "@/lib/utils";
import {
    InputOTP,
    InputOTPGroup,
    InputOTPSlot,
} from "@/components/ui/input-otp";
import { useEffect, useState, useCallback } from "react";
import useCommonState from "@/states/common";
import { useShallow } from "zustand/react/shallow";
import { toast } from "sonner";
import { formatError } from "@/helpers/util";
import { useTheme } from "next-themes";
import { getSystemTheme } from "@/helpers/util";

interface CaptchaProps {
    className?: string;
    onChange: (id: string, value: string) => void;
}

export function Captcha({ className, onChange }: CaptchaProps) {
    const [fetchCaptcha] = useCommonState(
        useShallow((state) => [state.fetchCaptcha]),
    );
    const { theme } = useTheme();
    const [captcha, setCaptcha] = useState({
        id: "",
        data: "",
    });
    const refreshCaptcha = useCallback(async () => {
        setCaptcha({
            id: "",
            data: "",
        });
        try {
            let currentTheme = theme || "";
            if (!currentTheme || currentTheme === "system") {
                currentTheme = getSystemTheme();
            }
            const data = await fetchCaptcha(currentTheme);
            setCaptcha(data);
        } catch (error) {
            toast.error(formatError(error));
            setCaptcha({
                id: "",
                data: "",
            });
        }
    }, [fetchCaptcha, theme]);

    useEffect(() => {
        refreshCaptcha();
    }, [refreshCaptcha]);

    return (
        <div className={cn("flex items-center gap-2", className)}>
            <InputOTP
                maxLength={4}
                onChange={(value) => {
                    if (captcha.id) {
                        onChange(captcha.id, value);
                    } else {
                        onChange("", "");
                    }
                }}
            >
                <InputOTPGroup>
                    <InputOTPSlot index={0} />
                    <InputOTPSlot index={1} />
                    <InputOTPSlot index={2} />
                    <InputOTPSlot index={3} />
                </InputOTPGroup>
            </InputOTP>
            <div>
                {captcha.id && (
                    <img
                        onClick={refreshCaptcha}
                        style={{
                            height: "32px",
                            cursor: "pointer",
                        }}
                        src={`data:image/png;base64,${captcha.data}`}
                        alt="captcha"
                    />
                )}
            </div>
        </div>
    );
}
