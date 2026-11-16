import { useCallback, useEffect, useMemo, useState } from "react";
import {
    PlusIcon,
    RefreshCwIcon,
    ShieldIcon,
    TrashIcon,
    XIcon,
} from "lucide-react";
import { toast } from "sonner";
import { useShallow } from "zustand/react/shallow";
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
import { RBAC_OVERVIEW, RBAC_PERMISSIONS, RBAC_ROLES } from "@/constants/url";
import request from "@/helpers/request";
import {
    formatDate,
    formatError,
    hasPermission,
    isAdminRole,
} from "@/helpers/util";
import { useI18n } from "@/i18n";
import useBasicState from "@/states/basic";
import useUserState from "@/states/user";

interface PermissionItem {
    id: number;
    code: string;
    description: string;
    created: string;
    modified: string;
}

interface RoleSummary {
    role: string;
    immutable: boolean;
    permissions: string[];
}

interface RbacOverviewResp {
    permissions: PermissionItem[];
    roles: RoleSummary[];
    can_write: boolean;
}

export default function Rbac() {
    const commonI18n = useI18n("common");
    const modelI18n = useI18n("model");
    const rbacI18n = useI18n("rbac");
    const [setPageHeaderNavigation, resetPageHeaderNavigation] = useBasicState(
        useShallow((state) => [
            state.setPageHeaderNavigation,
            state.resetPageHeaderNavigation,
        ]),
    );
    const [user, initialized] = useUserState(
        useShallow((state) => [state.data, state.initialized]),
    );

    const canRead =
        isAdminRole(user.roles) ||
        hasPermission(user.permissions, "rbac:read") ||
        hasPermission(user.permissions, "rbac:write");

    const [overview, setOverview] = useState<RbacOverviewResp | null>(null);
    const [refreshing, setRefreshing] = useState(false);
    const [busy, setBusy] = useState(false);

    const [grantRole, setGrantRole] = useState("admin");
    const [grantCode, setGrantCode] = useState("");
    const [permCode, setPermCode] = useState("");
    const [permDesc, setPermDesc] = useState("");
    const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

    const fetchOverview = useCallback(async () => {
        const { data } = await request.get<RbacOverviewResp>(RBAC_OVERVIEW);
        return data;
    }, []);

    const refreshData = async () => {
        if (!canRead) return;
        setRefreshing(true);
        try {
            const data = await fetchOverview();
            setOverview(data);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setRefreshing(false);
        }
    };

    useEffect(() => {
        setPageHeaderNavigation([
            {
                title: rbacI18n("title"),
                icon: <ShieldIcon className="size-4" />,
            },
        ]);
        return () => {
            resetPageHeaderNavigation();
        };
    }, [rbacI18n, setPageHeaderNavigation, resetPageHeaderNavigation]);

    useEffect(() => {
        if (!initialized || !canRead) return;
        let cancelled = false;
        fetchOverview()
            .then((data) => {
                if (!cancelled) {
                    setOverview(data);
                }
            })
            .catch((err) => {
                if (!cancelled) {
                    toast.error(formatError(err));
                }
            });
        return () => {
            cancelled = true;
        };
    }, [initialized, canRead, fetchOverview]);

    const selectablePermissions = useMemo(
        () =>
            (overview?.permissions ?? [])
                .map((p) => p.code)
                .filter((code) => code !== "*"),
        [overview],
    );

    const editableRoles = useMemo(
        () =>
            (overview?.roles ?? [])
                .filter((r) => !r.immutable)
                .map((r) => r.role),
        [overview],
    );

    const handleGrant = async () => {
        const role = grantRole.trim();
        const code = grantCode.trim();
        if (!role || !code || busy) return;
        setBusy(true);
        try {
            await request.post(
                `${RBAC_ROLES}/${encodeURIComponent(role)}/permissions`,
                { permission_code: code },
            );
            toast.success(rbacI18n("grantSuccess"));
            const data = await fetchOverview();
            setOverview(data);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setBusy(false);
        }
    };

    const handleRevoke = async (role: string, code: string) => {
        if (busy) return;
        setBusy(true);
        try {
            await request.delete(
                `${RBAC_ROLES}/${encodeURIComponent(role)}/permissions/${encodeURIComponent(code)}`,
            );
            toast.success(rbacI18n("revokeSuccess"));
            const data = await fetchOverview();
            setOverview(data);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setBusy(false);
        }
    };

    const handleSavePermission = async () => {
        const code = permCode.trim();
        if (!code || busy) return;
        setBusy(true);
        try {
            await request.post(RBAC_PERMISSIONS, {
                code,
                description: permDesc.trim(),
            });
            toast.success(rbacI18n("savePermSuccess"));
            setPermCode("");
            setPermDesc("");
            const data = await fetchOverview();
            setOverview(data);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setBusy(false);
        }
    };

    const handleDeletePermissionConfirm = async () => {
        if (!deleteTarget || busy) return;
        const target = deleteTarget;
        setDeleteTarget(null);
        setBusy(true);
        try {
            await request.delete(
                `${RBAC_PERMISSIONS}/${encodeURIComponent(target)}`,
            );
            toast.success(rbacI18n("deletePermSuccess"));
            const data = await fetchOverview();
            setOverview(data);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setBusy(false);
        }
    };

    if (!initialized || (canRead && !overview)) {
        return <Loading />;
    }

    if (!canRead) {
        return (
            <div className="p-6">
                <Card>
                    <CardContent className="pt-6 text-sm text-muted-foreground">
                        {commonI18n("noPermission")}
                    </CardContent>
                </Card>
            </div>
        );
    }

    const canWrite = Boolean(overview?.can_write);

    return (
        <div className="w-full space-y-6 p-2 md:p-4">
            <div className="flex items-center justify-between gap-2">
                <div>
                    <h1 className="text-xl font-semibold">
                        {rbacI18n("title")}
                    </h1>
                    {!canWrite && (
                        <p className="text-xs text-muted-foreground mt-1">
                            {rbacI18n("readOnlyNotice")}
                        </p>
                    )}
                </div>
                <Button
                    variant="outline"
                    size="sm"
                    disabled={refreshing}
                    onClick={() => void refreshData()}
                >
                    <RefreshCwIcon
                        className={`mr-2 size-4 ${refreshing ? "animate-spin" : ""}`}
                    />
                    {commonI18n("refresh")}
                </Button>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>{rbacI18n("rolesTitle")}</CardTitle>
                    <CardDescription>{rbacI18n("rolesDesc")}</CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    {canWrite && (
                        <div className="flex flex-col gap-2 sm:flex-row">
                            <Input
                                list="rbac-editable-roles"
                                className="sm:w-56"
                                value={grantRole}
                                onChange={(e) => setGrantRole(e.target.value)}
                                placeholder={rbacI18n("grantRolePlaceholder")}
                            />
                            <datalist id="rbac-editable-roles">
                                {editableRoles.map((r) => (
                                    <option key={r} value={r} />
                                ))}
                            </datalist>
                            <Select
                                value={grantCode}
                                onValueChange={setGrantCode}
                            >
                                <SelectTrigger className="w-full sm:w-64">
                                    <SelectValue
                                        placeholder={rbacI18n(
                                            "grantPermPlaceholder",
                                        )}
                                    />
                                </SelectTrigger>
                                <SelectContent>
                                    {selectablePermissions.map((code) => (
                                        <SelectItem key={code} value={code}>
                                            {code}
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                            <Button
                                type="button"
                                disabled={
                                    !grantRole.trim() ||
                                    !grantCode.trim() ||
                                    busy
                                }
                                onClick={() => void handleGrant()}
                            >
                                <PlusIcon className="mr-1.5 size-4" />
                                {rbacI18n("grantBtn")}
                            </Button>
                        </div>
                    )}

                    <div className="space-y-3">
                        {overview?.roles.map((roleItem) => (
                            <div
                                key={roleItem.role}
                                className="rounded-lg border p-3 space-y-2"
                            >
                                <div className="flex items-center justify-between gap-2">
                                    <div className="flex items-center gap-2">
                                        <Badge
                                            variant={
                                                roleItem.immutable
                                                    ? "default"
                                                    : "outline"
                                            }
                                        >
                                            {roleItem.role}
                                        </Badge>
                                        {roleItem.immutable && (
                                            <span className="text-xs text-muted-foreground">
                                                {rbacI18n("roleImmutable")}
                                            </span>
                                        )}
                                    </div>
                                    {canWrite && !roleItem.immutable && (
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="sm"
                                            onClick={() =>
                                                setGrantRole(roleItem.role)
                                            }
                                        >
                                            {rbacI18n("grantRoleLabel")}
                                        </Button>
                                    )}
                                </div>
                                <div className="flex flex-wrap gap-2">
                                    {roleItem.permissions.length === 0 ? (
                                        <span className="text-xs text-muted-foreground">
                                            {rbacI18n("roleEmptyPerms")}
                                        </span>
                                    ) : (
                                        roleItem.permissions.map((perm) => (
                                            <Badge
                                                key={perm}
                                                variant="secondary"
                                                className="gap-1 font-mono text-xs"
                                            >
                                                <span>{perm}</span>
                                                {canWrite &&
                                                    !roleItem.immutable && (
                                                        <button
                                                            type="button"
                                                            className="ml-1 rounded-full hover:text-destructive"
                                                            disabled={busy}
                                                            onClick={() =>
                                                                void handleRevoke(
                                                                    roleItem.role,
                                                                    perm,
                                                                )
                                                            }
                                                        >
                                                            <XIcon className="size-3" />
                                                        </button>
                                                    )}
                                            </Badge>
                                        ))
                                    )}
                                </div>
                            </div>
                        ))}
                    </div>
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>{rbacI18n("permissionsTitle")}</CardTitle>
                    <CardDescription>
                        {rbacI18n("permissionsDesc")}
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    {canWrite && (
                        <div className="flex flex-col gap-2 sm:flex-row">
                            <Input
                                className="sm:w-64 font-mono"
                                value={permCode}
                                onChange={(e) => setPermCode(e.target.value)}
                                placeholder={rbacI18n("codePlaceholder")}
                            />
                            <Input
                                className="flex-1"
                                value={permDesc}
                                onChange={(e) => setPermDesc(e.target.value)}
                                placeholder={rbacI18n("descPlaceholder")}
                            />
                            <Button
                                type="button"
                                disabled={!permCode.trim() || busy}
                                onClick={() => void handleSavePermission()}
                            >
                                {rbacI18n("savePermBtn")}
                            </Button>
                        </div>
                    )}

                    <div className="rounded-md border">
                        <Table>
                            <TableHeader>
                                <TableRow>
                                    <TableHead>{rbacI18n("colCode")}</TableHead>
                                    <TableHead>
                                        {rbacI18n("colDescription")}
                                    </TableHead>
                                    <TableHead>
                                        {rbacI18n("colModified")}
                                    </TableHead>
                                    {canWrite && (
                                        <TableHead className="w-24 text-right">
                                            {rbacI18n("colActions")}
                                        </TableHead>
                                    )}
                                </TableRow>
                            </TableHeader>
                            <TableBody>
                                {!overview?.permissions.length ? (
                                    <TableRow>
                                        <TableCell
                                            colSpan={canWrite ? 4 : 3}
                                            className="h-24 text-center text-muted-foreground"
                                        >
                                            {rbacI18n("emptyPermissions")}
                                        </TableCell>
                                    </TableRow>
                                ) : (
                                    overview.permissions.map((perm) => (
                                        <TableRow key={perm.id}>
                                            <TableCell className="font-mono text-xs font-medium">
                                                {perm.code}
                                            </TableCell>
                                            <TableCell className="text-sm">
                                                {perm.description || "-"}
                                            </TableCell>
                                            <TableCell className="text-xs">
                                                {formatDate(perm.modified)}
                                            </TableCell>
                                            {canWrite && (
                                                <TableCell className="text-right">
                                                    {perm.code === "*" ? (
                                                        <span className="text-xs text-muted-foreground">
                                                            -
                                                        </span>
                                                    ) : (
                                                        <Button
                                                            type="button"
                                                            variant="ghost"
                                                            size="sm"
                                                            disabled={busy}
                                                            onClick={() =>
                                                                setDeleteTarget(
                                                                    perm.code,
                                                                )
                                                            }
                                                        >
                                                            <TrashIcon className="mr-1 size-3.5 text-destructive" />
                                                            {rbacI18n(
                                                                "deleteBtn",
                                                            )}
                                                        </Button>
                                                    )}
                                                </TableCell>
                                            )}
                                        </TableRow>
                                    ))
                                )}
                            </TableBody>
                        </Table>
                    </div>
                </CardContent>
            </Card>

            <AlertDialog
                open={Boolean(deleteTarget)}
                onOpenChange={(open) => {
                    if (!open) setDeleteTarget(null);
                }}
            >
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>
                            {rbacI18n("deletePermTitle")}
                        </AlertDialogTitle>
                        <AlertDialogDescription>
                            {rbacI18n("deletePermDesc")}
                            {deleteTarget ? ` (${deleteTarget})` : ""}
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>
                            {modelI18n("cancel")}
                        </AlertDialogCancel>
                        <AlertDialogAction
                            onClick={() => void handleDeletePermissionConfirm()}
                        >
                            {modelI18n("continue")}
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </div>
    );
}
