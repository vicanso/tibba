import { useCallback, useEffect, useState } from "react";
import {
    ActivityIcon,
    EyeIcon,
    RefreshCwIcon,
    RotateCcwIcon,
    TrashIcon,
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
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
} from "@/components/ui/dialog";
import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import { JOBS_DEAD, JOBS_STATS } from "@/constants/url";
import HTTPError from "@/helpers/http-error";
import request from "@/helpers/request";
import { formatDate, formatError, isAdminRole } from "@/helpers/util";
import { useI18n } from "@/i18n";
import useBasicState from "@/states/basic";
import useUserState from "@/states/user";

interface QueueStats {
    pending: number;
    running: number;
    dead: number;
}

interface DeadJobItem {
    id: number;
    queue: string;
    job_type: string;
    payload: Record<string, unknown>;
    attempts: number;
    max_attempts: number;
    last_error?: string | null;
    run_at: string;
    created: string;
}

const PAGE_LIMIT = 20;

async function fetchJobsPayload(targetOffset: number) {
    const [statsRes, deadRes] = await Promise.all([
        request.get<QueueStats>(JOBS_STATS),
        request.get<DeadJobItem[]>(JOBS_DEAD, {
            params: {
                limit: PAGE_LIMIT,
                offset: targetOffset,
            },
        }),
    ]);
    return {
        stats: statsRes.data,
        deadJobs: deadRes.data,
    };
}

export default function Jobs() {
    const commonI18n = useI18n("common");
    const jobsI18n = useI18n("jobs");
    const modelI18n = useI18n("model");
    const [setPageHeaderNavigation, resetPageHeaderNavigation] = useBasicState(
        useShallow((state) => [
            state.setPageHeaderNavigation,
            state.resetPageHeaderNavigation,
        ]),
    );
    const [userInfo, initialized] = useUserState(
        useShallow((state) => [state.data, state.initialized]),
    );

    const [stats, setStats] = useState<QueueStats | null>(null);
    const [deadJobs, setDeadJobs] = useState<DeadJobItem[]>([]);
    const [offset, setOffset] = useState(0);
    const [loadedOffset, setLoadedOffset] = useState<number | null>(null);
    const [refreshing, setRefreshing] = useState(false);
    const [featureDisabled, setFeatureDisabled] = useState(false);
    const [payloadJob, setPayloadJob] = useState<DeadJobItem | null>(null);
    const [purgeJob, setPurgeJob] = useState<DeadJobItem | null>(null);

    const isAdmin = isAdminRole(userInfo.roles);
    const loading =
        refreshing ||
        (initialized && isAdmin && !featureDisabled && loadedOffset !== offset);

    const applyJobsSuccess = useCallback(
        (
            data: { stats: QueueStats; deadJobs: DeadJobItem[] },
            targetOffset: number,
        ) => {
            setStats(data.stats);
            setDeadJobs(data.deadJobs);
            setFeatureDisabled(false);
            setLoadedOffset(targetOffset);
        },
        [],
    );

    const applyJobsError = useCallback((err: unknown, targetOffset: number) => {
        setLoadedOffset(targetOffset);
        if (err instanceof HTTPError && err.status === 404) {
            setFeatureDisabled(true);
        } else {
            toast.error(formatError(err));
        }
    }, []);

    const refreshData = async () => {
        if (!isAdmin) return;
        setRefreshing(true);
        try {
            const data = await fetchJobsPayload(offset);
            applyJobsSuccess(data, offset);
        } catch (err) {
            applyJobsError(err, offset);
        } finally {
            setRefreshing(false);
        }
    };

    useEffect(() => {
        setPageHeaderNavigation([
            {
                title: jobsI18n("title"),
                icon: <ActivityIcon className="size-4" />,
            },
        ]);
        return () => {
            resetPageHeaderNavigation();
        };
    }, [jobsI18n, setPageHeaderNavigation, resetPageHeaderNavigation]);

    useEffect(() => {
        if (!initialized || !isAdmin) return;
        let cancelled = false;
        fetchJobsPayload(offset)
            .then((data) => {
                if (!cancelled) {
                    applyJobsSuccess(data, offset);
                }
            })
            .catch((err) => {
                if (!cancelled) {
                    applyJobsError(err, offset);
                }
            });
        return () => {
            cancelled = true;
        };
    }, [initialized, isAdmin, offset, applyJobsSuccess, applyJobsError]);

    const handleRetry = async (id: number) => {
        try {
            await request.post(`${JOBS_DEAD}/${id}/retry`);
            toast.success(jobsI18n("retrySuccess"));
            await refreshData();
        } catch (err) {
            toast.error(formatError(err));
        }
    };

    const handlePurgeConfirm = async () => {
        if (!purgeJob) return;
        const targetId = purgeJob.id;
        setPurgeJob(null);
        try {
            await request.delete(`${JOBS_DEAD}/${targetId}`);
            toast.success(jobsI18n("purgeSuccess"));
            await refreshData();
        } catch (err) {
            toast.error(formatError(err));
        }
    };

    if (!initialized) {
        return <Loading />;
    }

    if (!isAdmin) {
        return (
            <div className="p-6 text-center text-muted-foreground">
                {commonI18n("noPermission")}
            </div>
        );
    }

    if (featureDisabled) {
        return (
            <div className="p-6 text-center text-muted-foreground">
                {jobsI18n("featureDisabled")}
            </div>
        );
    }

    return (
        <div className="space-y-6 p-2 md:p-4">
            <div className="flex items-center justify-between">
                <h2 className="text-xl font-semibold">{jobsI18n("title")}</h2>
                <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={loading}
                    onClick={refreshData}
                >
                    <RefreshCwIcon
                        className={`mr-1.5 size-4 ${loading ? "animate-spin" : ""}`}
                    />
                    {jobsI18n("refresh")}
                </Button>
            </div>

            <div className="grid gap-4 sm:grid-cols-3">
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">
                            {jobsI18n("pending")}
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold">
                            {stats ? stats.pending : "-"}
                        </div>
                    </CardContent>
                </Card>
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">
                            {jobsI18n("running")}
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold">
                            {stats ? stats.running : "-"}
                        </div>
                    </CardContent>
                </Card>
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">
                            {jobsI18n("dead")}
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold text-destructive">
                            {stats ? stats.dead : "-"}
                        </div>
                    </CardContent>
                </Card>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>{jobsI18n("deadListTitle")}</CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="rounded-md border">
                        <Table>
                            <TableHeader>
                                <TableRow>
                                    <TableHead>{jobsI18n("id")}</TableHead>
                                    <TableHead>{jobsI18n("queue")}</TableHead>
                                    <TableHead>{jobsI18n("jobType")}</TableHead>
                                    <TableHead>
                                        {jobsI18n("attempts")}
                                    </TableHead>
                                    <TableHead>
                                        {jobsI18n("lastError")}
                                    </TableHead>
                                    <TableHead>{jobsI18n("runAt")}</TableHead>
                                    <TableHead>{jobsI18n("created")}</TableHead>
                                    <TableHead className="text-right">
                                        {jobsI18n("actions")}
                                    </TableHead>
                                </TableRow>
                            </TableHeader>
                            <TableBody>
                                {deadJobs.length === 0 ? (
                                    <TableRow>
                                        <TableCell
                                            colSpan={8}
                                            className="h-24 text-center text-muted-foreground"
                                        >
                                            {jobsI18n("empty")}
                                        </TableCell>
                                    </TableRow>
                                ) : (
                                    deadJobs.map((job) => (
                                        <TableRow key={job.id}>
                                            <TableCell className="font-mono text-xs">
                                                {job.id}
                                            </TableCell>
                                            <TableCell>
                                                <Badge variant="outline">
                                                    {job.queue}
                                                </Badge>
                                            </TableCell>
                                            <TableCell className="font-mono text-xs">
                                                {job.job_type}
                                            </TableCell>
                                            <TableCell className="text-xs">
                                                {job.attempts} /{" "}
                                                {job.max_attempts}
                                            </TableCell>
                                            <TableCell className="max-w-[280px] truncate text-xs text-destructive">
                                                {job.last_error || "-"}
                                            </TableCell>
                                            <TableCell className="text-xs">
                                                {formatDate(job.run_at)}
                                            </TableCell>
                                            <TableCell className="text-xs">
                                                {formatDate(job.created)}
                                            </TableCell>
                                            <TableCell className="text-right">
                                                <div className="flex justify-end gap-1">
                                                    <Button
                                                        type="button"
                                                        variant="ghost"
                                                        size="sm"
                                                        onClick={() =>
                                                            setPayloadJob(job)
                                                        }
                                                    >
                                                        <EyeIcon className="mr-1 size-3.5" />
                                                        {jobsI18n(
                                                            "viewPayload",
                                                        )}
                                                    </Button>
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        size="sm"
                                                        onClick={() =>
                                                            handleRetry(job.id)
                                                        }
                                                    >
                                                        <RotateCcwIcon className="mr-1 size-3.5" />
                                                        {jobsI18n("retry")}
                                                    </Button>
                                                    <Button
                                                        type="button"
                                                        variant="destructive"
                                                        size="sm"
                                                        onClick={() =>
                                                            setPurgeJob(job)
                                                        }
                                                    >
                                                        <TrashIcon className="mr-1 size-3.5" />
                                                        {jobsI18n("purge")}
                                                    </Button>
                                                </div>
                                            </TableCell>
                                        </TableRow>
                                    ))
                                )}
                            </TableBody>
                        </Table>
                    </div>

                    <div className="flex items-center justify-end gap-2">
                        <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            disabled={offset === 0 || loading}
                            onClick={() =>
                                setOffset((prev) =>
                                    Math.max(0, prev - PAGE_LIMIT),
                                )
                            }
                        >
                            {jobsI18n("previousPage")}
                        </Button>
                        <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            disabled={deadJobs.length < PAGE_LIMIT || loading}
                            onClick={() =>
                                setOffset((prev) => prev + PAGE_LIMIT)
                            }
                        >
                            {jobsI18n("nextPage")}
                        </Button>
                    </div>
                </CardContent>
            </Card>

            <Dialog
                open={Boolean(payloadJob)}
                onOpenChange={(open) => {
                    if (!open) setPayloadJob(null);
                }}
            >
                <DialogContent className="max-w-2xl">
                    <DialogHeader>
                        <DialogTitle>
                            {jobsI18n("payloadTitle")}
                            {payloadJob ? ` #${payloadJob.id}` : ""}
                        </DialogTitle>
                    </DialogHeader>
                    <pre className="max-h-[60vh] overflow-auto rounded-md border bg-muted p-4 font-mono text-xs whitespace-pre-wrap break-all">
                        {payloadJob
                            ? JSON.stringify(payloadJob.payload, null, 2)
                            : ""}
                    </pre>
                </DialogContent>
            </Dialog>

            <AlertDialog
                open={Boolean(purgeJob)}
                onOpenChange={(open) => {
                    if (!open) setPurgeJob(null);
                }}
            >
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>
                            {jobsI18n("purgeTitle")}
                        </AlertDialogTitle>
                        <AlertDialogDescription>
                            {jobsI18n("purgeDesc")}
                            {purgeJob
                                ? ` (#${purgeJob.id} ${purgeJob.job_type})`
                                : ""}
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>
                            {modelI18n("cancel")}
                        </AlertDialogCancel>
                        <AlertDialogAction onClick={handlePurgeConfirm}>
                            {modelI18n("continue")}
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </div>
    );
}
