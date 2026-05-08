"use client";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
    Card,
    CardContent,
    CardDescription,
    CardHeader,
    CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useI18n } from "@/i18n";
import { Link, useLocation } from "react-router";
import { HOME, SIGN_UP } from "@/constants/route";
import { Captcha } from "@/components/captcha";
import { useState } from "react";
import useUserState from "@/states/user";
import { toast } from "sonner";
import { formatError } from "@/helpers/util";
import { useShallow } from "zustand/react/shallow";
import router from "@/routers";
import { nanoid } from "nanoid";

export function LoginForm({
    className,
    ...props
}: React.ComponentPropsWithoutRef<"div">) {
    const location = useLocation();
    const i18nLogin = useI18n("login");
    const defaultAccount = location.state?.account || "";
    const [account, setAccount] = useState(defaultAccount);
    const [password, setPassword] = useState("");
    const [captcha, setCaptcha] = useState("");
    const [processing, setProcessing] = useState(false);
    const [login] = useUserState(useShallow((state) => [state.login]));
    const [captchaId, setCaptchaId] = useState(nanoid());
    const canSubmit = () => {
        return account && password && captcha;
    };

    const handleLogin = async () => {
        if (processing) {
            return;
        }
        setProcessing(true);
        try {
            await login(account, password, captcha);
            router.navigate(HOME);
        } catch (err) {
            toast.error(formatError(err));
            setCaptchaId(nanoid());
        } finally {
            setProcessing(false);
        }
    };

    return (
        <div className={cn("flex flex-col gap-6", className)} {...props}>
            <Card>
                <CardHeader>
                    <CardTitle className="text-2xl">
                        {i18nLogin("title")}
                    </CardTitle>
                    <CardDescription>
                        {i18nLogin("description")}
                    </CardDescription>
                </CardHeader>
                <CardContent>
                    <form>
                        <div className="flex flex-col gap-6">
                            <div className="grid gap-2">
                                <Label htmlFor="account">
                                    {i18nLogin("account")}
                                </Label>
                                <Input
                                    id="account"
                                    placeholder={i18nLogin(
                                        "accountPlaceholder",
                                    )}
                                    autoFocus={!defaultAccount}
                                    defaultValue={defaultAccount}
                                    onChange={(e) => {
                                        setAccount(e.target.value);
                                    }}
                                    required
                                />
                            </div>
                            <div className="grid gap-2">
                                <div className="flex items-center">
                                    <Label htmlFor="password">
                                        {i18nLogin("password")}
                                    </Label>
                                    <a
                                        tabIndex={-1}
                                        href="#"
                                        className="ml-auto inline-block text-sm underline-offset-4 hover:underline"
                                    >
                                        {i18nLogin("forgotPassword")}
                                    </a>
                                </div>
                                <Input
                                    id="password"
                                    type="password"
                                    autoFocus={defaultAccount != ""}
                                    required
                                    placeholder={i18nLogin(
                                        "passwordPlaceholder",
                                    )}
                                    onChange={(e) => {
                                        setPassword(e.target.value);
                                    }}
                                />
                            </div>
                            <div className="grid gap-2">
                                <div className="flex items-center">
                                    <Label htmlFor="captcha">
                                        {i18nLogin("captcha")}
                                    </Label>
                                </div>
                                <Captcha
                                    key={captchaId}
                                    onChange={(id, value) => {
                                        setCaptcha(`${id}:${value}`);
                                    }}
                                />
                            </div>
                            <Button
                                type="submit"
                                className="w-full"
                                disabled={!canSubmit()}
                                onClick={(e) => {
                                    e.preventDefault();
                                    handleLogin();
                                }}
                            >
                                {i18nLogin("submit")}
                            </Button>
                        </div>
                        <div className="mt-4 text-center text-sm">
                            {i18nLogin("dontHaveAccount")}{" "}
                            <Link
                                to={SIGN_UP}
                                className="underline underline-offset-4"
                            >
                                {i18nLogin("signUp")}
                            </Link>
                        </div>
                    </form>
                </CardContent>
            </Card>
        </div>
    );
}
