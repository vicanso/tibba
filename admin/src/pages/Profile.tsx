import { useEffect, useState } from "react";
import { Link } from "react-router";
import { useShallow } from "zustand/react/shallow";
import {
    CopyIcon,
    ExternalLinkIcon,
    KeyIcon,
    Loader2Icon,
    MailCheckIcon,
    ShieldCheckIcon,
    TrashIcon,
    UserIcon,
} from "lucide-react";
import { toast } from "sonner";
import { ProfileForm } from "@/components/profile-form";
import { Loading } from "@/components/loading";
import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
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
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import { LOGIN } from "@/constants/route";
import HTTPError from "@/helpers/http-error";
import { formatDate, formatError } from "@/helpers/util";
import { useI18n } from "@/i18n";
import useBasicState from "@/states/basic";
import useUserState, {
    ApiKeyItem,
    CreateApiKeyResp,
    TotpEnrollResp,
    TotpStatus,
} from "@/states/user";

export default function Profile() {
    const commonI18n = useI18n("common");
    const homeI18n = useI18n("home");
    const modelI18n = useI18n("model");
    const profileI18n = useI18n("profile");
    const [setPageHeaderNavigation, resetPageHeaderNavigation] = useBasicState(
        useShallow((state) => [
            state.setPageHeaderNavigation,
            state.resetPageHeaderNavigation,
        ]),
    );
    const [
        user,
        initialized,
        requestEmailVerify,
        confirmEmailVerify,
        getTotpStatus,
        enrollTotp,
        activateTotp,
        disableTotp,
        listApiKeys,
        createApiKey,
        revokeApiKey,
    ] = useUserState(
        useShallow((state) => [
            state.data,
            state.initialized,
            state.requestEmailVerify,
            state.confirmEmailVerify,
            state.getTotpStatus,
            state.enrollTotp,
            state.activateTotp,
            state.disableTotp,
            state.listApiKeys,
            state.createApiKey,
            state.revokeApiKey,
        ]),
    );

    const [emailToken, setEmailToken] = useState("");
    const [sendingEmail, setSendingEmail] = useState(false);
    const [confirmingEmail, setConfirmingEmail] = useState(false);

    const [totpStatus, setTotpStatus] = useState<TotpStatus | null>(null);
    const [totpSupported, setTotpSupported] = useState(true);
    const [enrollData, setEnrollData] = useState<TotpEnrollResp | null>(null);
    const [totpCode, setTotpCode] = useState("");
    const [disableCode, setDisableCode] = useState("");
    const [recoveryCodes, setRecoveryCodes] = useState<string[]>([]);
    const [totpBusy, setTotpBusy] = useState(false);

    const [apiKeys, setApiKeys] = useState<ApiKeyItem[]>([]);
    const [apiKeySupported, setApiKeySupported] = useState(true);
    const [newKeyName, setNewKeyName] = useState("");
    const [newKeyExpire, setNewKeyExpire] = useState("never");
    const [createdKey, setCreatedKey] = useState<CreateApiKeyResp | null>(null);
    const [apiKeyBusy, setApiKeyBusy] = useState(false);
    const [revokeTarget, setRevokeTarget] = useState<ApiKeyItem | null>(null);

    useEffect(() => {
        setPageHeaderNavigation([
            {
                title: profileI18n("title"),
                icon: <UserIcon className="size-4" />,
            },
        ]);
        return () => {
            resetPageHeaderNavigation();
        };
    }, [profileI18n, setPageHeaderNavigation, resetPageHeaderNavigation]);

    useEffect(() => {
        if (!user.account) {
            return;
        }
        let cancelled = false;
        getTotpStatus()
            .then((status) => {
                if (!cancelled) {
                    setTotpStatus(status);
                    setTotpSupported(true);
                }
            })
            .catch((err) => {
                if (cancelled) return;
                if (err instanceof HTTPError && err.status === 404) {
                    setTotpSupported(false);
                } else {
                    toast.error(formatError(err));
                }
            });

        listApiKeys()
            .then((keys) => {
                if (!cancelled) {
                    setApiKeys(keys);
                    setApiKeySupported(true);
                }
            })
            .catch((err) => {
                if (cancelled) return;
                if (err instanceof HTTPError && err.status === 404) {
                    setApiKeySupported(false);
                } else {
                    toast.error(formatError(err));
                }
            });

        return () => {
            cancelled = true;
        };
    }, [user.account, getTotpStatus, listApiKeys]);

    const copyText = async (text: string) => {
        try {
            await navigator.clipboard.writeText(text);
            toast.success(commonI18n("copied"));
        } catch {
            toast.error("Copy failed");
        }
    };

    const handleSendVerifyEmail = async () => {
        if (sendingEmail) return;
        setSendingEmail(true);
        try {
            await requestEmailVerify();
            toast.success(profileI18n("verifyEmailSent"));
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setSendingEmail(false);
        }
    };

    const handleConfirmVerifyEmail = async () => {
        if (confirmingEmail || !emailToken.trim()) return;
        setConfirmingEmail(true);
        try {
            await confirmEmailVerify(emailToken);
            toast.success(profileI18n("emailVerifiedSuccess"));
            setEmailToken("");
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setConfirmingEmail(false);
        }
    };

    const handleEnrollTotp = async () => {
        if (totpBusy) return;
        setTotpBusy(true);
        try {
            const resp = await enrollTotp();
            setEnrollData(resp);
            setRecoveryCodes([]);
            setTotpStatus({ enrolled: true, enabled: false });
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setTotpBusy(false);
        }
    };

    const handleActivateTotp = async () => {
        if (totpBusy || !totpCode.trim()) return;
        setTotpBusy(true);
        try {
            const resp = await activateTotp(totpCode);
            setRecoveryCodes(resp.recovery_codes);
            setEnrollData(null);
            setTotpCode("");
            setTotpStatus({ enrolled: true, enabled: true });
            toast.success(profileI18n("totpActivateSuccess"));
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setTotpBusy(false);
        }
    };

    const handleDisableTotp = async () => {
        if (totpBusy || !disableCode.trim()) return;
        setTotpBusy(true);
        try {
            await disableTotp(disableCode);
            setDisableCode("");
            setRecoveryCodes([]);
            setEnrollData(null);
            setTotpStatus({ enrolled: false, enabled: false });
            toast.success(profileI18n("totpDisableSuccess"));
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setTotpBusy(false);
        }
    };

    const handleCreateApiKey = async () => {
        if (apiKeyBusy || !newKeyName.trim()) return;
        setApiKeyBusy(true);
        try {
            const days =
                newKeyExpire === "never" ? undefined : Number(newKeyExpire);
            const created = await createApiKey(newKeyName.trim(), days);
            setCreatedKey(created);
            setNewKeyName("");
            const keys = await listApiKeys();
            setApiKeys(keys);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setApiKeyBusy(false);
        }
    };

    const handleRevokeApiKey = async () => {
        if (!revokeTarget) return;
        const targetId = revokeTarget.id;
        setRevokeTarget(null);
        try {
            await revokeApiKey(targetId);
            toast.success(profileI18n("apiKeyRevokeSuccess"));
            const keys = await listApiKeys();
            setApiKeys(keys);
        } catch (err) {
            toast.error(formatError(err));
        }
    };

    if (!initialized) {
        return <Loading />;
    }

    if (!user.account) {
        return (
            <div className="mx-auto w-full max-w-xl p-6">
                <Card>
                    <CardHeader>
                        <CardTitle>{profileI18n("title")}</CardTitle>
                        <CardDescription>
                            {profileI18n("loginRequired")}
                        </CardDescription>
                    </CardHeader>
                    <CardContent>
                        <Button asChild>
                            <Link to={LOGIN}>{homeI18n("goLogin")}</Link>
                        </Button>
                    </CardContent>
                </Card>
            </div>
        );
    }

    return (
        <div className="mx-auto w-full max-w-4xl space-y-6 p-4 md:p-6">
            <Card>
                <CardHeader>
                    <CardTitle>{profileI18n("basicTitle")}</CardTitle>
                </CardHeader>
                <CardContent>
                    <ProfileForm />
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle className="flex items-center gap-2">
                        <MailCheckIcon className="size-5" />
                        {profileI18n("emailVerifyTitle")}
                    </CardTitle>
                    <CardDescription>
                        {profileI18n("emailVerifyDesc")}
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    {!user.email ? (
                        <p className="text-sm text-muted-foreground">
                            {profileI18n("emailVerifyMissing")}
                        </p>
                    ) : (
                        <>
                            <div className="flex flex-wrap items-center gap-3">
                                <Badge variant="secondary">{user.email}</Badge>
                                <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    disabled={sendingEmail}
                                    onClick={handleSendVerifyEmail}
                                >
                                    {sendingEmail && (
                                        <Loader2Icon className="mr-2 size-4 animate-spin" />
                                    )}
                                    {profileI18n("sendVerifyEmail")}
                                </Button>
                            </div>
                            <div className="flex flex-col gap-2 sm:flex-row">
                                <Input
                                    value={emailToken}
                                    onChange={(e) =>
                                        setEmailToken(e.target.value)
                                    }
                                    placeholder={profileI18n(
                                        "emailVerifyTokenPlaceholder",
                                    )}
                                />
                                <Button
                                    type="button"
                                    disabled={
                                        !emailToken.trim() || confirmingEmail
                                    }
                                    onClick={handleConfirmVerifyEmail}
                                >
                                    {confirmingEmail && (
                                        <Loader2Icon className="mr-2 size-4 animate-spin" />
                                    )}
                                    {profileI18n("confirmVerifyEmail")}
                                </Button>
                            </div>
                        </>
                    )}
                </CardContent>
            </Card>

            {totpSupported && (
                <Card>
                    <CardHeader>
                        <div className="flex items-center justify-between gap-2">
                            <div>
                                <CardTitle className="flex items-center gap-2">
                                    <ShieldCheckIcon className="size-5" />
                                    {profileI18n("totpTitle")}
                                </CardTitle>
                                <CardDescription className="mt-1.5">
                                    {profileI18n("totpDesc")}
                                </CardDescription>
                            </div>
                            <Badge
                                variant={
                                    totpStatus?.enabled
                                        ? "default"
                                        : "secondary"
                                }
                            >
                                {totpStatus?.enabled
                                    ? profileI18n("totpEnabled")
                                    : totpStatus?.enrolled
                                      ? profileI18n("totpEnrolledPending")
                                      : profileI18n("totpDisabled")}
                            </Badge>
                        </div>
                    </CardHeader>
                    <CardContent className="space-y-4">
                        {recoveryCodes.length > 0 && (
                            <div className="space-y-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-4">
                                <div className="flex items-center justify-between gap-2">
                                    <h4 className="font-medium text-sm">
                                        {profileI18n("totpRecoveryTitle")}
                                    </h4>
                                    <Button
                                        type="button"
                                        variant="outline"
                                        size="sm"
                                        onClick={() =>
                                            copyText(recoveryCodes.join("\n"))
                                        }
                                    >
                                        <CopyIcon className="mr-1.5 size-3.5" />
                                        {profileI18n("totpCopyAll")}
                                    </Button>
                                </div>
                                <p className="text-xs text-muted-foreground">
                                    {profileI18n("totpRecoveryWarning")}
                                </p>
                                <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
                                    {recoveryCodes.map((code) => (
                                        <code
                                            key={code}
                                            className="rounded border bg-background px-2 py-1 text-center font-mono text-xs"
                                        >
                                            {code}
                                        </code>
                                    ))}
                                </div>
                            </div>
                        )}

                        {!totpStatus?.enabled ? (
                            <div className="space-y-4">
                                <Button
                                    type="button"
                                    variant="outline"
                                    disabled={totpBusy}
                                    onClick={handleEnrollTotp}
                                >
                                    {totpBusy && (
                                        <Loader2Icon className="mr-2 size-4 animate-spin" />
                                    )}
                                    {profileI18n("totpSetupBtn")}
                                </Button>

                                {enrollData && (
                                    <div className="space-y-4 rounded-lg border p-4">
                                        <div className="space-y-1.5">
                                            <Label>
                                                {profileI18n("totpSecretLabel")}
                                            </Label>
                                            <div className="flex items-center gap-2">
                                                <code className="flex-1 break-all rounded border bg-muted px-3 py-2 font-mono text-sm">
                                                    {enrollData.secret}
                                                </code>
                                                <Button
                                                    type="button"
                                                    variant="outline"
                                                    size="sm"
                                                    onClick={() =>
                                                        copyText(
                                                            enrollData.secret,
                                                        )
                                                    }
                                                >
                                                    <CopyIcon className="mr-1.5 size-3.5" />
                                                    {commonI18n("copy")}
                                                </Button>
                                            </div>
                                        </div>

                                        <div className="space-y-1.5">
                                            <Label>
                                                {profileI18n("totpUriLabel")}
                                            </Label>
                                            <div className="flex flex-wrap items-center gap-2">
                                                <code className="min-w-0 flex-1 truncate rounded border bg-muted px-3 py-2 font-mono text-xs">
                                                    {enrollData.otpauth_uri}
                                                </code>
                                                <Button
                                                    type="button"
                                                    variant="outline"
                                                    size="sm"
                                                    onClick={() =>
                                                        copyText(
                                                            enrollData.otpauth_uri,
                                                        )
                                                    }
                                                >
                                                    <CopyIcon className="mr-1.5 size-3.5" />
                                                    {commonI18n("copy")}
                                                </Button>
                                                <Button
                                                    type="button"
                                                    variant="secondary"
                                                    size="sm"
                                                    asChild
                                                >
                                                    <a
                                                        href={
                                                            enrollData.otpauth_uri
                                                        }
                                                    >
                                                        <ExternalLinkIcon className="mr-1.5 size-3.5" />
                                                        {profileI18n(
                                                            "totpOpenApp",
                                                        )}
                                                    </a>
                                                </Button>
                                            </div>
                                        </div>

                                        <div className="space-y-1.5">
                                            <Label>
                                                {profileI18n("totpCodeLabel")}
                                            </Label>
                                            <div className="flex flex-col gap-2 sm:flex-row">
                                                <Input
                                                    value={totpCode}
                                                    onChange={(e) =>
                                                        setTotpCode(
                                                            e.target.value,
                                                        )
                                                    }
                                                    placeholder={profileI18n(
                                                        "totpCodePlaceholder",
                                                    )}
                                                />
                                                <Button
                                                    type="button"
                                                    disabled={
                                                        !totpCode.trim() ||
                                                        totpBusy
                                                    }
                                                    onClick={handleActivateTotp}
                                                >
                                                    {totpBusy && (
                                                        <Loader2Icon className="mr-2 size-4 animate-spin" />
                                                    )}
                                                    {profileI18n(
                                                        "totpActivateBtn",
                                                    )}
                                                </Button>
                                            </div>
                                        </div>
                                    </div>
                                )}
                            </div>
                        ) : (
                            <div className="flex flex-col gap-2 sm:flex-row">
                                <Input
                                    value={disableCode}
                                    onChange={(e) =>
                                        setDisableCode(e.target.value)
                                    }
                                    placeholder={profileI18n(
                                        "totpDisableCodePlaceholder",
                                    )}
                                />
                                <Button
                                    type="button"
                                    variant="destructive"
                                    disabled={!disableCode.trim() || totpBusy}
                                    onClick={handleDisableTotp}
                                >
                                    {totpBusy && (
                                        <Loader2Icon className="mr-2 size-4 animate-spin" />
                                    )}
                                    {profileI18n("totpDisableBtn")}
                                </Button>
                            </div>
                        )}
                    </CardContent>
                </Card>
            )}

            {apiKeySupported && (
                <Card>
                    <CardHeader>
                        <CardTitle className="flex items-center gap-2">
                            <KeyIcon className="size-5" />
                            {profileI18n("apiKeyTitle")}
                        </CardTitle>
                        <CardDescription>
                            {profileI18n("apiKeyDesc")}
                        </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-4">
                        {createdKey && (
                            <div className="space-y-2 rounded-lg border border-emerald-500/40 bg-emerald-500/10 p-4">
                                <div className="flex items-center justify-between gap-2">
                                    <span className="font-medium text-sm">
                                        {profileI18n("apiKeyCreatedBanner")}
                                    </span>
                                    <Button
                                        type="button"
                                        variant="ghost"
                                        size="sm"
                                        onClick={() => setCreatedKey(null)}
                                    >
                                        {commonI18n("close")}
                                    </Button>
                                </div>
                                <div className="flex items-center gap-2">
                                    <code className="flex-1 break-all rounded border bg-background px-3 py-2 font-mono text-xs">
                                        {createdKey.token}
                                    </code>
                                    <Button
                                        type="button"
                                        variant="outline"
                                        size="sm"
                                        onClick={() =>
                                            copyText(createdKey.token)
                                        }
                                    >
                                        <CopyIcon className="mr-1.5 size-3.5" />
                                        {commonI18n("copy")}
                                    </Button>
                                </div>
                            </div>
                        )}

                        <div className="flex flex-col gap-2 sm:flex-row">
                            <Input
                                value={newKeyName}
                                onChange={(e) => setNewKeyName(e.target.value)}
                                placeholder={profileI18n(
                                    "apiKeyNamePlaceholder",
                                )}
                            />
                            <Select
                                value={newKeyExpire}
                                onValueChange={setNewKeyExpire}
                            >
                                <SelectTrigger className="w-full sm:w-44">
                                    <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="never">
                                        {profileI18n("apiKeyExpireNever")}
                                    </SelectItem>
                                    <SelectItem value="30">
                                        {profileI18n("apiKeyExpire30")}
                                    </SelectItem>
                                    <SelectItem value="90">
                                        {profileI18n("apiKeyExpire90")}
                                    </SelectItem>
                                    <SelectItem value="365">
                                        {profileI18n("apiKeyExpire365")}
                                    </SelectItem>
                                </SelectContent>
                            </Select>
                            <Button
                                type="button"
                                disabled={!newKeyName.trim() || apiKeyBusy}
                                onClick={handleCreateApiKey}
                            >
                                {apiKeyBusy && (
                                    <Loader2Icon className="mr-2 size-4 animate-spin" />
                                )}
                                {profileI18n("apiKeyCreateBtn")}
                            </Button>
                        </div>

                        <div className="rounded-md border">
                            <Table>
                                <TableHeader>
                                    <TableRow>
                                        <TableHead>
                                            {profileI18n("apiKeyColName")}
                                        </TableHead>
                                        <TableHead>
                                            {profileI18n("apiKeyColPrefix")}
                                        </TableHead>
                                        <TableHead>
                                            {profileI18n("apiKeyColLastUsed")}
                                        </TableHead>
                                        <TableHead>
                                            {profileI18n("apiKeyColExpires")}
                                        </TableHead>
                                        <TableHead>
                                            {profileI18n("apiKeyColCreated")}
                                        </TableHead>
                                        <TableHead className="w-24 text-right">
                                            {profileI18n("apiKeyColActions")}
                                        </TableHead>
                                    </TableRow>
                                </TableHeader>
                                <TableBody>
                                    {apiKeys.length === 0 ? (
                                        <TableRow>
                                            <TableCell
                                                colSpan={6}
                                                className="h-20 text-center text-muted-foreground"
                                            >
                                                {profileI18n("apiKeyEmpty")}
                                            </TableCell>
                                        </TableRow>
                                    ) : (
                                        apiKeys.map((item) => (
                                            <TableRow key={item.id}>
                                                <TableCell className="font-medium">
                                                    {item.name}
                                                </TableCell>
                                                <TableCell className="font-mono text-xs">
                                                    {item.key_prefix}...
                                                </TableCell>
                                                <TableCell className="text-xs">
                                                    {item.last_used_at
                                                        ? formatDate(
                                                              item.last_used_at,
                                                          )
                                                        : "-"}
                                                </TableCell>
                                                <TableCell className="text-xs">
                                                    {item.expires_at
                                                        ? formatDate(
                                                              item.expires_at,
                                                          )
                                                        : profileI18n(
                                                              "apiKeyExpireNever",
                                                          )}
                                                </TableCell>
                                                <TableCell className="text-xs">
                                                    {formatDate(item.created)}
                                                </TableCell>
                                                <TableCell className="text-right">
                                                    <Button
                                                        type="button"
                                                        variant="ghost"
                                                        size="sm"
                                                        onClick={() =>
                                                            setRevokeTarget(
                                                                item,
                                                            )
                                                        }
                                                    >
                                                        <TrashIcon className="mr-1 size-3.5 text-destructive" />
                                                        {profileI18n(
                                                            "apiKeyRevoke",
                                                        )}
                                                    </Button>
                                                </TableCell>
                                            </TableRow>
                                        ))
                                    )}
                                </TableBody>
                            </Table>
                        </div>
                    </CardContent>
                </Card>
            )}

            <AlertDialog
                open={Boolean(revokeTarget)}
                onOpenChange={(open) => {
                    if (!open) setRevokeTarget(null);
                }}
            >
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>
                            {profileI18n("apiKeyRevokeTitle")}
                        </AlertDialogTitle>
                        <AlertDialogDescription>
                            {profileI18n("apiKeyRevokeDesc")}
                            {revokeTarget ? ` (${revokeTarget.name})` : ""}
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>
                            {modelI18n("cancel")}
                        </AlertDialogCancel>
                        <AlertDialogAction onClick={handleRevokeApiKey}>
                            {modelI18n("continue")}
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </div>
    );
}
