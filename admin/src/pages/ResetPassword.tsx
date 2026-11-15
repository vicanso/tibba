import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router";
import { KeyRoundIcon, Loader2Icon } from "lucide-react";
import { toast } from "sonner";
import { useShallow } from "zustand/react/shallow";
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
import { LOGIN } from "@/constants/route";
import { formatError } from "@/helpers/util";
import { useI18n } from "@/i18n";
import useBasicState from "@/states/basic";
import useUserState from "@/states/user";

export default function ResetPassword() {
    const i18nReset = useI18n("resetPassword");
    const navigate = useNavigate();
    const [setPageHeaderNavigation, resetPageHeaderNavigation] = useBasicState(
        useShallow((state) => [
            state.setPageHeaderNavigation,
            state.resetPageHeaderNavigation,
        ]),
    );
    const [requestPasswordReset, confirmPasswordReset] = useUserState(
        useShallow((state) => [
            state.requestPasswordReset,
            state.confirmPasswordReset,
        ]),
    );

    const [step, setStep] = useState<"request" | "confirm">("request");
    const [account, setAccount] = useState("");
    const [token, setToken] = useState("");
    const [password, setPassword] = useState("");
    const [confirmPassword, setConfirmPassword] = useState("");
    const [processing, setProcessing] = useState(false);

    useEffect(() => {
        setPageHeaderNavigation([
            {
                title: i18nReset("title"),
                icon: <KeyRoundIcon className="size-4" />,
            },
        ]);
        return () => {
            resetPageHeaderNavigation();
        };
    }, [i18nReset, setPageHeaderNavigation, resetPageHeaderNavigation]);

    const handleRequestReset = async () => {
        if (processing || !account.trim()) {
            return;
        }
        setProcessing(true);
        try {
            await requestPasswordReset(account.trim());
            toast.success(i18nReset("requestSent"));
            setStep("confirm");
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setProcessing(false);
        }
    };

    const handleConfirmReset = async () => {
        if (
            processing ||
            !token.trim() ||
            password.length < 6 ||
            password !== confirmPassword
        ) {
            return;
        }
        setProcessing(true);
        try {
            await confirmPasswordReset(token.trim(), password);
            toast.success(i18nReset("success"));
            navigate(LOGIN, {
                state: account.trim() ? { account: account.trim() } : undefined,
            });
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setProcessing(false);
        }
    };

    return (
        <div className="flex w-full items-center justify-center p-6 md:p-10">
            <div className="w-full max-w-sm">
                <Card>
                    <CardHeader>
                        <CardTitle className="text-2xl">
                            {i18nReset("title")}
                        </CardTitle>
                        <CardDescription>
                            {step === "request"
                                ? i18nReset("stepRequestDesc")
                                : i18nReset("stepConfirmDesc")}
                        </CardDescription>
                    </CardHeader>
                    <CardContent>
                        {step === "request" ? (
                            <form
                                onSubmit={(e) => {
                                    e.preventDefault();
                                    handleRequestReset();
                                }}
                                className="flex flex-col gap-4"
                            >
                                <div className="grid gap-2">
                                    <Label htmlFor="reset-account">
                                        {i18nReset("account")}
                                    </Label>
                                    <Input
                                        id="reset-account"
                                        autoFocus
                                        required
                                        value={account}
                                        placeholder={i18nReset(
                                            "accountPlaceholder",
                                        )}
                                        onChange={(e) =>
                                            setAccount(e.target.value)
                                        }
                                    />
                                </div>
                                <Button
                                    type="submit"
                                    className="w-full"
                                    disabled={!account.trim() || processing}
                                >
                                    {processing && (
                                        <Loader2Icon className="mr-2 size-4 animate-spin" />
                                    )}
                                    {i18nReset("sendCode")}
                                </Button>
                                <Button
                                    type="button"
                                    variant="ghost"
                                    className="w-full"
                                    onClick={() => setStep("confirm")}
                                >
                                    {i18nReset("hasToken")}
                                </Button>
                            </form>
                        ) : (
                            <form
                                onSubmit={(e) => {
                                    e.preventDefault();
                                    handleConfirmReset();
                                }}
                                className="flex flex-col gap-4"
                            >
                                <div className="grid gap-2">
                                    <Label htmlFor="reset-token">
                                        {i18nReset("token")}
                                    </Label>
                                    <Input
                                        id="reset-token"
                                        autoFocus
                                        required
                                        value={token}
                                        placeholder={i18nReset(
                                            "tokenPlaceholder",
                                        )}
                                        onChange={(e) =>
                                            setToken(e.target.value)
                                        }
                                    />
                                </div>
                                <div className="grid gap-2">
                                    <Label htmlFor="reset-password">
                                        {i18nReset("newPassword")}
                                    </Label>
                                    <Input
                                        id="reset-password"
                                        type="password"
                                        required
                                        value={password}
                                        placeholder={i18nReset(
                                            "newPasswordPlaceholder",
                                        )}
                                        onChange={(e) =>
                                            setPassword(e.target.value)
                                        }
                                    />
                                </div>
                                <div className="grid gap-2">
                                    <Label htmlFor="reset-confirm-password">
                                        {i18nReset("confirmPassword")}
                                    </Label>
                                    <Input
                                        id="reset-confirm-password"
                                        type="password"
                                        required
                                        value={confirmPassword}
                                        placeholder={i18nReset(
                                            "confirmPasswordPlaceholder",
                                        )}
                                        onChange={(e) =>
                                            setConfirmPassword(e.target.value)
                                        }
                                    />
                                </div>
                                <Button
                                    type="submit"
                                    className="w-full"
                                    disabled={
                                        !token.trim() ||
                                        password.length < 6 ||
                                        password !== confirmPassword ||
                                        processing
                                    }
                                >
                                    {processing && (
                                        <Loader2Icon className="mr-2 size-4 animate-spin" />
                                    )}
                                    {i18nReset("submit")}
                                </Button>
                                <Button
                                    type="button"
                                    variant="ghost"
                                    className="w-full"
                                    onClick={() => setStep("request")}
                                >
                                    {i18nReset("requestAgain")}
                                </Button>
                            </form>
                        )}
                        <div className="mt-4 text-center text-sm">
                            <Link
                                to={LOGIN}
                                className="underline underline-offset-4"
                            >
                                {i18nReset("backToLogin")}
                            </Link>
                        </div>
                    </CardContent>
                </Card>
            </div>
        </div>
    );
}
