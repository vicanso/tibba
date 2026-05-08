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
import { Link } from "react-router";
import { LOGIN } from "@/constants/route";
import { useState } from "react";
import { Loader2Icon } from "lucide-react";
import { toast } from "sonner";
import { formatError } from "@/helpers/util";
import useUserState from "@/states/user";
import { useShallow } from "zustand/react/shallow";
import router from "@/routers";

export function SignUpForm({
    className,
    ...props
}: React.ComponentPropsWithoutRef<"div">) {
    const i18nSignUp = useI18n("signUp");
    const [account, setAccount] = useState("");
    const [password, setPassword] = useState("");
    const [confirmPassword, setConfirmPassword] = useState("");
    const [processing, setProcessing] = useState(false);
    const [signUp] = useUserState(useShallow((state) => [state.signUp]));
    const canSubmit = () => {
        return account && password && password == confirmPassword;
    };
    const handleSignUp = async () => {
        if (processing) {
            return;
        }
        setProcessing(true);
        try {
            await signUp(account, password);
            toast.success(i18nSignUp("success"));
            router.navigate(LOGIN, {
                state: {
                    account,
                },
            });
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setProcessing(false);
        }
    };

    return (
        <div className={cn("flex flex-col gap-6", className)} {...props}>
            <Card>
                <CardHeader>
                    <CardTitle className="text-2xl">
                        {i18nSignUp("title")}
                    </CardTitle>
                    <CardDescription>
                        {i18nSignUp("description")}
                    </CardDescription>
                </CardHeader>
                <CardContent>
                    <form>
                        <div className="flex flex-col gap-6">
                            <div className="grid gap-2">
                                <Label htmlFor="account">
                                    {i18nSignUp("account")}
                                </Label>
                                <Input
                                    id="account"
                                    autoFocus
                                    placeholder={i18nSignUp(
                                        "accountPlaceholder",
                                    )}
                                    required
                                    value={account}
                                    onChange={(e) => setAccount(e.target.value)}
                                />
                            </div>
                            <div className="grid gap-2">
                                <div className="flex items-center">
                                    <Label htmlFor="password">
                                        {i18nSignUp("password")}
                                    </Label>
                                </div>
                                <Input
                                    id="password"
                                    type="password"
                                    required
                                    placeholder={i18nSignUp(
                                        "passwordPlaceholder",
                                    )}
                                    value={password}
                                    onChange={(e) =>
                                        setPassword(e.target.value)
                                    }
                                />
                            </div>
                            <div className="grid gap-2">
                                <div className="flex items-center">
                                    <Label htmlFor="confirmPassword">
                                        {i18nSignUp("confirmPassword")}
                                    </Label>
                                </div>
                                <Input
                                    id="confirmPassword"
                                    type="password"
                                    required
                                    placeholder={i18nSignUp(
                                        "confirmPasswordPlaceholder",
                                    )}
                                    value={confirmPassword}
                                    onChange={(e) =>
                                        setConfirmPassword(e.target.value)
                                    }
                                />
                            </div>
                            <Button
                                type="submit"
                                className="w-full"
                                disabled={!canSubmit()}
                                onClick={handleSignUp}
                            >
                                {processing && (
                                    <Loader2Icon className="size-4 animate-spin mr-2" />
                                )}
                                {i18nSignUp("submit")}
                            </Button>
                        </div>
                        <div className="mt-4 text-center text-sm">
                            {i18nSignUp("haveAccount")}{" "}
                            <Link
                                to={LOGIN}
                                className="underline underline-offset-4"
                            >
                                {i18nSignUp("login")}
                            </Link>
                        </div>
                    </form>
                </CardContent>
            </Card>
        </div>
    );
}
