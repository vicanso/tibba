import { useCallback, useEffect, useState } from "react";
import dayjs from "dayjs";
import {
    EyeIcon,
    RefreshCwIcon,
    RotateCcwIcon,
    ScrollTextIcon,
    SearchIcon,
} from "lucide-react";
import { toast } from "sonner";
import { useShallow } from "zustand/react/shallow";
import { DateTimePicker } from "@/components/date-time-picker";
import { Loading } from "@/components/loading";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import { AUDIT_LOGS_TRACE, AUDIT_LOGS_URL } from "@/constants/url";
import request from "@/helpers/request";
import {
    formatDate,
    formatError,
    hasPermission,
    isAdminRole,
} from "@/helpers/util";
import { useI18n } from "@/i18n";
import useBasicState from "@/states/basic";
import useUserState, { AuditLogItem } from "@/states/user";

interface AuditLogListResp {
    count: number;
    audit_logs: AuditLogItem[];
}

interface FilterDraft {
    userId: string;
    action: string;
    targetType: string;
    requestId: string;
    ip: string;
    startTime: string;
    endTime: string;
}

const emptyFilters: FilterDraft = {
    userId: "",
    action: "",
    targetType: "",
    requestId: "",
    ip: "",
    startTime: "",
    endTime: "",
};

const PAGE_LIMIT = 20;

export default function AuditLogs() {
    const commonI18n = useI18n("common");
    const auditI18n = useI18n("auditLogs");
    const componentI18n = useI18n("component");
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
        hasPermission(user.permissions, "audit:read");

    const [draft, setDraft] = useState<FilterDraft>(emptyFilters);
    const [applied, setApplied] = useState<FilterDraft>(emptyFilters);
    const [page, setPage] = useState(1);

    const [items, setItems] = useState<AuditLogItem[] | null>(null);
    const [total, setTotal] = useState(0);
    const [refreshing, setRefreshing] = useState(false);

    const [detailItem, setDetailItem] = useState<AuditLogItem | null>(null);
    const [traceReqId, setTraceReqId] = useState<string | null>(null);
    const [traceItems, setTraceItems] = useState<AuditLogItem[]>([]);
    const [traceLoading, setTraceLoading] = useState(false);

    const fetchPage = useCallback(
        async (targetPage: number, filters: FilterDraft) => {
            const params: Record<string, string | number | boolean> = {
                page: targetPage,
                limit: PAGE_LIMIT,
                count: true,
            };
            const parsedUserId = filters.userId.trim();
            if (parsedUserId && /^-?\d+$/.test(parsedUserId)) {
                params.user_id = Number(parsedUserId);
            }
            if (filters.action.trim()) params.action = filters.action.trim();
            if (filters.targetType.trim()) {
                params.target_type = filters.targetType.trim();
            }
            if (filters.requestId.trim()) {
                params.request_id = filters.requestId.trim();
            }
            if (filters.ip.trim()) params.ip = filters.ip.trim();
            if (filters.startTime.trim()) {
                params.start_time = filters.startTime.trim();
            }
            if (filters.endTime.trim()) {
                params.end_time = filters.endTime.trim();
            }

            const { data } = await request.get<AuditLogListResp>(
                AUDIT_LOGS_URL,
                { params },
            );
            return data;
        },
        [],
    );

    const refreshData = async () => {
        if (!canRead) return;
        setRefreshing(true);
        try {
            const data = await fetchPage(page, applied);
            setItems(data.audit_logs);
            setTotal(data.count);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setRefreshing(false);
        }
    };

    useEffect(() => {
        setPageHeaderNavigation([
            {
                title: auditI18n("title"),
                icon: <ScrollTextIcon className="size-4" />,
            },
        ]);
        return () => {
            resetPageHeaderNavigation();
        };
    }, [auditI18n, setPageHeaderNavigation, resetPageHeaderNavigation]);

    useEffect(() => {
        if (!initialized || !canRead) return;
        let cancelled = false;
        fetchPage(page, applied)
            .then((data) => {
                if (!cancelled) {
                    setItems(data.audit_logs);
                    setTotal(data.count);
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
    }, [initialized, canRead, page, applied, fetchPage]);

    const openTrace = async (reqId: string) => {
        setTraceReqId(reqId);
        setTraceItems([]);
        setTraceLoading(true);
        try {
            const { data } = await request.get<AuditLogItem[]>(
                `${AUDIT_LOGS_TRACE}/${encodeURIComponent(reqId)}`,
            );
            setTraceItems(data);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setTraceLoading(false);
        }
    };

    const handleSearch = () => {
        setPage(1);
        setApplied({ ...draft });
    };

    const handleReset = () => {
        setDraft(emptyFilters);
        setPage(1);
        setApplied(emptyFilters);
    };

    if (!initialized || (canRead && items === null)) {
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

    const totalPages =
        total >= 0 ? Math.max(1, Math.ceil(total / PAGE_LIMIT)) : page;

    return (
        <div className="w-full space-y-6 p-2 md:p-4">
            <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-3">
                    <h1 className="text-xl font-semibold">
                        {auditI18n("title")}
                    </h1>
                    {total >= 0 && (
                        <Badge variant="secondary">
                            {auditI18n("totalLabel").replace(
                                "{total}",
                                String(total),
                            )}
                        </Badge>
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
                    <CardTitle className="text-base">
                        {auditI18n("search")}
                    </CardTitle>
                </CardHeader>
                <CardContent className="space-y-3">
                    <div className="grid gap-2 sm:grid-cols-2 md:grid-cols-4">
                        <Input
                            value={draft.userId}
                            onChange={(e) =>
                                setDraft((d) => ({
                                    ...d,
                                    userId: e.target.value,
                                }))
                            }
                            placeholder={auditI18n("userIdPlaceholder")}
                        />
                        <Input
                            value={draft.action}
                            onChange={(e) =>
                                setDraft((d) => ({
                                    ...d,
                                    action: e.target.value,
                                }))
                            }
                            placeholder={auditI18n("actionPlaceholder")}
                        />
                        <Input
                            value={draft.targetType}
                            onChange={(e) =>
                                setDraft((d) => ({
                                    ...d,
                                    targetType: e.target.value,
                                }))
                            }
                            placeholder={auditI18n("targetTypePlaceholder")}
                        />
                        <Input
                            value={draft.requestId}
                            onChange={(e) =>
                                setDraft((d) => ({
                                    ...d,
                                    requestId: e.target.value,
                                }))
                            }
                            placeholder={auditI18n("requestIdPlaceholder")}
                        />
                        <Input
                            value={draft.ip}
                            onChange={(e) =>
                                setDraft((d) => ({ ...d, ip: e.target.value }))
                            }
                            placeholder={auditI18n("ipPlaceholder")}
                        />
                        <DateTimePicker
                            date={draft.startTime || undefined}
                            clearable
                            i18n={componentI18n}
                            placeholder={auditI18n("startTimePlaceholder")}
                            setDate={(date) =>
                                setDraft((d) => ({
                                    ...d,
                                    startTime: date
                                        ? dayjs(date).second(0).toISOString()
                                        : "",
                                }))
                            }
                        />
                        <DateTimePicker
                            date={draft.endTime || undefined}
                            clearable
                            i18n={componentI18n}
                            placeholder={auditI18n("endTimePlaceholder")}
                            setDate={(date) =>
                                setDraft((d) => ({
                                    ...d,
                                    endTime: date
                                        ? dayjs(date).second(59).toISOString()
                                        : "",
                                }))
                            }
                        />
                        <div className="flex items-center gap-2">
                            <Button type="button" onClick={handleSearch}>
                                <SearchIcon className="mr-1.5 size-4" />
                                {auditI18n("search")}
                            </Button>
                            <Button
                                type="button"
                                variant="outline"
                                onClick={handleReset}
                            >
                                <RotateCcwIcon className="mr-1.5 size-4" />
                                {auditI18n("reset")}
                            </Button>
                        </div>
                    </div>
                </CardContent>
            </Card>

            <Card>
                <CardContent className="pt-6 space-y-4">
                    <div className="rounded-md border">
                        <Table>
                            <TableHeader>
                                <TableRow>
                                    <TableHead>{auditI18n("colId")}</TableHead>
                                    <TableHead>
                                        {auditI18n("colTime")}
                                    </TableHead>
                                    <TableHead>
                                        {auditI18n("colUserId")}
                                    </TableHead>
                                    <TableHead>
                                        {auditI18n("colAction")}
                                    </TableHead>
                                    <TableHead>
                                        {auditI18n("colTarget")}
                                    </TableHead>
                                    <TableHead>{auditI18n("colIp")}</TableHead>
                                    <TableHead>
                                        {auditI18n("colRequestId")}
                                    </TableHead>
                                    <TableHead className="text-right">
                                        {auditI18n("colActions")}
                                    </TableHead>
                                </TableRow>
                            </TableHeader>
                            <TableBody>
                                {!items?.length ? (
                                    <TableRow>
                                        <TableCell
                                            colSpan={8}
                                            className="h-24 text-center text-muted-foreground"
                                        >
                                            {auditI18n("empty")}
                                        </TableCell>
                                    </TableRow>
                                ) : (
                                    items.map((item) => (
                                        <TableRow key={item.id}>
                                            <TableCell className="font-mono text-xs">
                                                {item.id}
                                            </TableCell>
                                            <TableCell className="text-xs whitespace-nowrap">
                                                {formatDate(item.created)}
                                            </TableCell>
                                            <TableCell className="font-mono text-xs">
                                                {item.user_id ?? "-"}
                                            </TableCell>
                                            <TableCell>
                                                <Badge variant="outline">
                                                    {item.action}
                                                </Badge>
                                            </TableCell>
                                            <TableCell className="font-mono text-xs">
                                                {item.target_type ||
                                                item.target_id
                                                    ? `${item.target_type}:${item.target_id}`
                                                    : "-"}
                                            </TableCell>
                                            <TableCell className="font-mono text-xs">
                                                {item.ip || "-"}
                                            </TableCell>
                                            <TableCell className="font-mono text-xs">
                                                {item.request_id ? (
                                                    <Button
                                                        type="button"
                                                        variant="link"
                                                        size="sm"
                                                        className="h-auto p-0 font-mono text-xs"
                                                        onClick={() =>
                                                            void openTrace(
                                                                item.request_id,
                                                            )
                                                        }
                                                    >
                                                        {item.request_id}
                                                    </Button>
                                                ) : (
                                                    "-"
                                                )}
                                            </TableCell>
                                            <TableCell className="text-right">
                                                <Button
                                                    type="button"
                                                    variant="ghost"
                                                    size="sm"
                                                    onClick={() =>
                                                        setDetailItem(item)
                                                    }
                                                >
                                                    <EyeIcon className="mr-1 size-3.5" />
                                                    {auditI18n("viewDetail")}
                                                </Button>
                                            </TableCell>
                                        </TableRow>
                                    ))
                                )}
                            </TableBody>
                        </Table>
                    </div>

                    <div className="flex items-center justify-between gap-2">
                        <span className="text-xs text-muted-foreground">
                            {auditI18n("pageInfo")
                                .replace("{page}", String(page))
                                .replace("{total}", String(totalPages))}
                        </span>
                        <div className="flex items-center gap-2">
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                disabled={page <= 1}
                                onClick={() =>
                                    setPage((p) => Math.max(1, p - 1))
                                }
                            >
                                {auditI18n("previousPage")}
                            </Button>
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                disabled={page >= totalPages}
                                onClick={() => setPage((p) => p + 1)}
                            >
                                {auditI18n("nextPage")}
                            </Button>
                        </div>
                    </div>
                </CardContent>
            </Card>

            <Dialog
                open={Boolean(detailItem)}
                onOpenChange={(open) => {
                    if (!open) setDetailItem(null);
                }}
            >
                <DialogContent className="max-w-2xl">
                    <DialogHeader>
                        <DialogTitle>{auditI18n("detailTitle")}</DialogTitle>
                    </DialogHeader>
                    {detailItem && (
                        <div className="space-y-3 text-xs">
                            <div>
                                <p className="font-medium mb-1">
                                    {auditI18n("userAgent")}
                                </p>
                                <p className="rounded border bg-muted p-2 font-mono break-all">
                                    {detailItem.user_agent || "-"}
                                </p>
                            </div>
                            <div>
                                <p className="font-medium mb-1">
                                    {auditI18n("detailJson")}
                                </p>
                                <pre className="max-h-80 overflow-auto rounded border bg-muted p-3 font-mono">
                                    {JSON.stringify(detailItem.detail, null, 2)}
                                </pre>
                            </div>
                        </div>
                    )}
                </DialogContent>
            </Dialog>

            <Dialog
                open={Boolean(traceReqId)}
                onOpenChange={(open) => {
                    if (!open) setTraceReqId(null);
                }}
            >
                <DialogContent className="max-w-3xl">
                    <DialogHeader>
                        <DialogTitle>
                            {auditI18n("traceTitle")} ({traceReqId})
                        </DialogTitle>
                    </DialogHeader>
                    {traceLoading ? (
                        <p className="py-6 text-center text-sm text-muted-foreground">
                            {commonI18n("loading")}
                        </p>
                    ) : traceItems.length === 0 ? (
                        <p className="py-6 text-center text-sm text-muted-foreground">
                            {auditI18n("traceEmpty")}
                        </p>
                    ) : (
                        <div className="max-h-96 overflow-auto rounded-md border">
                            <Table>
                                <TableHeader>
                                    <TableRow>
                                        <TableHead>
                                            {auditI18n("colTime")}
                                        </TableHead>
                                        <TableHead>
                                            {auditI18n("colUserId")}
                                        </TableHead>
                                        <TableHead>
                                            {auditI18n("colAction")}
                                        </TableHead>
                                        <TableHead>
                                            {auditI18n("colTarget")}
                                        </TableHead>
                                        <TableHead>
                                            {auditI18n("detailJson")}
                                        </TableHead>
                                    </TableRow>
                                </TableHeader>
                                <TableBody>
                                    {traceItems.map((item) => (
                                        <TableRow key={item.id}>
                                            <TableCell className="text-xs whitespace-nowrap">
                                                {formatDate(item.created)}
                                            </TableCell>
                                            <TableCell className="font-mono text-xs">
                                                {item.user_id ?? "-"}
                                            </TableCell>
                                            <TableCell>
                                                <Badge variant="outline">
                                                    {item.action}
                                                </Badge>
                                            </TableCell>
                                            <TableCell className="font-mono text-xs">
                                                {item.target_type ||
                                                item.target_id
                                                    ? `${item.target_type}:${item.target_id}`
                                                    : "-"}
                                            </TableCell>
                                            <TableCell className="font-mono text-xs">
                                                {JSON.stringify(item.detail)}
                                            </TableCell>
                                        </TableRow>
                                    ))}
                                </TableBody>
                            </Table>
                        </div>
                    )}
                </DialogContent>
            </Dialog>
        </div>
    );
}
