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
import { Link, useLocation, useNavigate, useSearchParams } from "react-router";
import { HOME, RESET_PASSWORD, SIGN_UP } from "@/constants/route";
import { USER_OAUTH_GITHUB, USER_OAUTH_GOOGLE } from "@/constants/url";
import { useState } from "react";
import useUserState from "@/states/user";
import { toast } from "sonner";
import { formatError } from "@/helpers/util";
import { useShallow } from "zustand/react/shallow";
import { Loader2Icon, ShieldCheckIcon } from "lucide-react";

export function LoginForm({
    className,
    ...props
}: React.ComponentPropsWithoutRef<"div">) {
    const location = useLocation();
    const navigate = useNavigate();
    const [searchParams, setSearchParams] = useSearchParams();
    const i18nLogin = useI18n("login");
    const defaultAccount =
        (location.state as { account?: string } | null)?.account || "";
    const [account, setAccount] = useState(defaultAccount);
    const [password, setPassword] = useState("");
    const [mfaCode, setMfaCode] = useState("");
    const [localMfaToken, setLocalMfaToken] = useState("");
    const [processing, setProcessing] = useState(false);

    const [login, loginMfa] = useUserState(
        useShallow((state) => [state.login, state.loginMfa]),
    );

    const urlMfaToken =
        searchParams.get("mfa_required") === "1"
            ? searchParams.get("mfa_token") || ""
            : "";
    const activeMfaToken = localMfaToken || urlMfaToken;

    const canSubmit = () => Boolean(account && password);

    const clearMfaState = () => {
        setLocalMfaToken("");
        setMfaCode("");
        if (urlMfaToken) {
            const next = new URLSearchParams(searchParams);
            next.delete("mfa_required");
            next.delete("mfa_token");
            setSearchParams(next, { replace: true });
        }
    };

    const handleLogin = async () => {
        if (processing || !canSubmit()) {
            return;
        }
        setProcessing(true);
        try {
            const result = await login(account, password);
            if (result.mfaRequired) {
                setLocalMfaToken(result.mfaToken);
                setMfaCode("");
                return;
            }
            navigate(HOME);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setProcessing(false);
        }
    };

    const handleMfaVerify = async () => {
        if (processing || !activeMfaToken || !mfaCode.trim()) {
            return;
        }
        setProcessing(true);
        try {
            await loginMfa(activeMfaToken, mfaCode);
            clearMfaState();
            navigate(HOME);
        } catch (err) {
            toast.error(formatError(err), {
                description: i18nLogin("mfaRetryNotice"),
            });
            // 后端 /users/login/mfa 一次性消费 mfa_token，失败后需回到登录步重新发起挑战
            clearMfaState();
        } finally {
            setProcessing(false);
        }
    };

    if (activeMfaToken) {
        return (
            <div className={cn("flex flex-col gap-6", className)} {...props}>
                <Card>
                    <CardHeader>
                        <CardTitle className="flex items-center gap-2 text-2xl">
                            <ShieldCheckIcon className="size-5" />
                            {i18nLogin("mfaTitle")}
                        </CardTitle>
                        <CardDescription>
                            {i18nLogin("mfaDescription")}
                        </CardDescription>
                    </CardHeader>
                    <CardContent>
                        <form
                            onSubmit={(e) => {
                                e.preventDefault();
                                handleMfaVerify();
                            }}
                        >
                            <div className="flex flex-col gap-6">
                                <div className="grid gap-2">
                                    <Label htmlFor="mfaCode">
                                        {i18nLogin("mfaCode")}
                                    </Label>
                                    <Input
                                        id="mfaCode"
                                        autoFocus
                                        autoComplete="one-time-code"
                                        placeholder={i18nLogin(
                                            "mfaCodePlaceholder",
                                        )}
                                        value={mfaCode}
                                        onChange={(e) =>
                                            setMfaCode(e.target.value)
                                        }
                                        required
                                    />
                                </div>
                                <Button
                                    type="submit"
                                    className="w-full"
                                    disabled={!mfaCode.trim() || processing}
                                >
                                    {processing && (
                                        <Loader2Icon className="mr-2 size-4 animate-spin" />
                                    )}
                                    {i18nLogin("mfaVerify")}
                                </Button>
                                <Button
                                    type="button"
                                    variant="ghost"
                                    className="w-full"
                                    onClick={clearMfaState}
                                >
                                    {i18nLogin("backToLogin")}
                                </Button>
                            </div>
                        </form>
                    </CardContent>
                </Card>
            </div>
        );
    }

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
                    <form
                        onSubmit={(e) => {
                            e.preventDefault();
                            handleLogin();
                        }}
                    >
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
                                    value={account}
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
                                    <Link
                                        tabIndex={-1}
                                        to={RESET_PASSWORD}
                                        className="ml-auto inline-block text-sm underline-offset-4 hover:underline"
                                    >
                                        {i18nLogin("forgotPassword")}
                                    </Link>
                                </div>
                                <Input
                                    id="password"
                                    type="password"
                                    autoFocus={defaultAccount !== ""}
                                    required
                                    placeholder={i18nLogin(
                                        "passwordPlaceholder",
                                    )}
                                    value={password}
                                    onChange={(e) => {
                                        setPassword(e.target.value);
                                    }}
                                />
                            </div>
                            <Button
                                type="submit"
                                className="w-full"
                                disabled={!canSubmit() || processing}
                            >
                                {processing && (
                                    <Loader2Icon className="mr-2 size-4 animate-spin" />
                                )}
                                {i18nLogin("submit")}
                            </Button>

                            <div className="relative text-center text-xs after:absolute after:inset-0 after:top-1/2 after:z-0 after:flex after:items-center after:border-t after:border-border">
                                <span className="relative z-10 bg-card px-2 text-muted-foreground">
                                    {i18nLogin("oauthDivider")}
                                </span>
                            </div>

                            <div className="grid grid-cols-2 gap-3">
                                <Button variant="outline" asChild>
                                    <a href={USER_OAUTH_GITHUB}>
                                        {i18nLogin("oauthGithub")}
                                    </a>
                                </Button>
                                <Button variant="outline" asChild>
                                    <a href={USER_OAUTH_GOOGLE}>
                                        {i18nLogin("oauthGoogle")}
                                    </a>
                                </Button>
                            </div>
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
