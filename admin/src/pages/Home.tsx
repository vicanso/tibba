import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router";
import {
    ActivityIcon,
    HomeIcon,
    RefreshCwIcon,
    ServerIcon,
    ShieldIcon,
    UploadIcon,
    UserIcon,
} from "lucide-react";
import { toast } from "sonner";
import { useShallow } from "zustand/react/shallow";
import { Loading } from "@/components/loading";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
    Card,
    CardContent,
    CardDescription,
    CardHeader,
    CardTitle,
} from "@/components/ui/card";
import { FILE_UPLOADER, JOBS, LOGIN, PROFILE } from "@/constants/route";
import { COMMON_APPLICATION, JOBS_STATS } from "@/constants/url";
import HTTPError from "@/helpers/http-error";
import request from "@/helpers/request";
import { formatError, isAdminRole } from "@/helpers/util";
import { useI18n } from "@/i18n";
import useBasicState from "@/states/basic";
import useUserState from "@/states/user";

interface ApplicationInfo {
    uptime: string;
    env: string;
    os: string;
    arch: string;
    commit_id: string;
    hostname: string;
    memory_usage_mb: number;
    cpu_usage: number;
    open_files: number;
    total_written_mb: number;
    total_read_mb: number;
    running: boolean;
}

interface JobQueueStats {
    pending: number;
    running: number;
    dead: number;
}

async function fetchAdminMetricsPayload() {
    return Promise.allSettled([
        request.get<ApplicationInfo>(COMMON_APPLICATION),
        request.get<JobQueueStats>(JOBS_STATS),
    ]);
}

export default function Home() {
    const commonI18n = useI18n("common");
    const homeI18n = useI18n("home");
    const [setPageHeaderNavigation, resetPageHeaderNavigation] = useBasicState(
        useShallow((state) => [
            state.setPageHeaderNavigation,
            state.resetPageHeaderNavigation,
        ]),
    );
    const [userInfo, initialized] = useUserState(
        useShallow((state) => [state.data, state.initialized]),
    );

    const [appInfo, setAppInfo] = useState<ApplicationInfo | null>(null);
    const [jobStats, setJobStats] = useState<JobQueueStats | null>(null);
    const [jobSupported, setJobSupported] = useState(true);
    const [loadingAdmin, setLoadingAdmin] = useState(false);

    const isAdmin = isAdminRole(userInfo.roles);

    const applyMetricsPayload = useCallback(
        ([appRes, jobRes]: Awaited<
            ReturnType<typeof fetchAdminMetricsPayload>
        >) => {
            if (appRes.status === "fulfilled") {
                setAppInfo(appRes.value.data);
            } else {
                toast.error(formatError(appRes.reason));
            }

            if (jobRes.status === "fulfilled") {
                setJobStats(jobRes.value.data);
                setJobSupported(true);
            } else {
                const err = jobRes.reason;
                if (err instanceof HTTPError && err.status === 404) {
                    setJobSupported(false);
                } else {
                    toast.error(formatError(err));
                }
            }
        },
        [],
    );

    const handleRefreshMetrics = async () => {
        if (!isAdmin) return;
        setLoadingAdmin(true);
        try {
            const payload = await fetchAdminMetricsPayload();
            applyMetricsPayload(payload);
        } finally {
            setLoadingAdmin(false);
        }
    };

    useEffect(() => {
        setPageHeaderNavigation([
            {
                title: homeI18n("title"),
                icon: <HomeIcon className="size-4" />,
            },
        ]);
        return () => {
            resetPageHeaderNavigation();
        };
    }, [homeI18n, setPageHeaderNavigation, resetPageHeaderNavigation]);

    useEffect(() => {
        if (!isAdmin || !userInfo.account) return;
        let cancelled = false;
        void fetchAdminMetricsPayload().then((payload) => {
            if (!cancelled) {
                applyMetricsPayload(payload);
            }
        });
        return () => {
            cancelled = true;
        };
    }, [isAdmin, userInfo.account, applyMetricsPayload]);

    if (!initialized) {
        return <Loading />;
    }

    if (!userInfo.account) {
        return (
            <div className="mx-auto w-full max-w-xl p-6">
                <Card>
                    <CardHeader>
                        <CardTitle>{homeI18n("guestTitle")}</CardTitle>
                        <CardDescription>
                            {homeI18n("guestDesc")}
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
        <div className="w-full space-y-6 p-2 md:p-4">
            <Card>
                <CardHeader className="flex flex-row items-center justify-between gap-4">
                    <div>
                        <CardTitle className="flex items-center gap-2">
                            <ShieldIcon className="size-5" />
                            {homeI18n("welcomeTitle")}
                        </CardTitle>
                        <CardDescription className="mt-1">
                            {userInfo.nickname || userInfo.account}
                        </CardDescription>
                    </div>
                    <div className="flex flex-wrap gap-2">
                        <Button variant="outline" size="sm" asChild>
                            <Link to={PROFILE}>
                                <UserIcon className="mr-1.5 size-4" />
                                {homeI18n("goProfile")}
                            </Link>
                        </Button>
                        <Button variant="outline" size="sm" asChild>
                            <Link to={FILE_UPLOADER}>
                                <UploadIcon className="mr-1.5 size-4" />
                                {homeI18n("goUpload")}
                            </Link>
                        </Button>
                    </div>
                </CardHeader>
                <CardContent className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                    <div className="rounded-lg border p-3">
                        <div className="text-xs text-muted-foreground">
                            {homeI18n("account")}
                        </div>
                        <div className="mt-1 font-medium">
                            {userInfo.account}
                        </div>
                    </div>
                    <div className="rounded-lg border p-3">
                        <div className="text-xs text-muted-foreground">
                            {homeI18n("roles")}
                        </div>
                        <div className="mt-1 flex flex-wrap gap-1">
                            {userInfo.roles.length > 0 ? (
                                userInfo.roles.map((r) => (
                                    <Badge key={r} variant="secondary">
                                        {r}
                                    </Badge>
                                ))
                            ) : (
                                <span className="text-sm text-muted-foreground">
                                    -
                                </span>
                            )}
                        </div>
                    </div>
                    <div className="rounded-lg border p-3">
                        <div className="text-xs text-muted-foreground">
                            {homeI18n("groups")}
                        </div>
                        <div className="mt-1 flex flex-wrap gap-1">
                            {userInfo.groups.length > 0 ? (
                                userInfo.groups.map((g) => (
                                    <Badge key={g} variant="outline">
                                        {g}
                                    </Badge>
                                ))
                            ) : (
                                <span className="text-sm text-muted-foreground">
                                    -
                                </span>
                            )}
                        </div>
                    </div>
                    <div className="rounded-lg border p-3">
                        <div className="text-xs text-muted-foreground">
                            {homeI18n("permissionCount")}
                        </div>
                        <div className="mt-1 text-xl font-semibold">
                            {userInfo.permissions.length}
                        </div>
                    </div>
                </CardContent>
            </Card>

            {isAdmin && (
                <>
                    <Card>
                        <CardHeader className="flex flex-row items-center justify-between gap-2">
                            <div className="flex items-center gap-2">
                                <ServerIcon className="size-5" />
                                <CardTitle>
                                    {homeI18n("appInfoTitle")}
                                </CardTitle>
                                {appInfo && (
                                    <Badge
                                        variant={
                                            appInfo.running
                                                ? "default"
                                                : "destructive"
                                        }
                                    >
                                        {appInfo.running
                                            ? homeI18n("running")
                                            : homeI18n("stopped")}
                                    </Badge>
                                )}
                            </div>
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                disabled={loadingAdmin}
                                onClick={handleRefreshMetrics}
                            >
                                <RefreshCwIcon
                                    className={`mr-1.5 size-3.5 ${loadingAdmin ? "animate-spin" : ""}`}
                                />
                                {commonI18n("refresh")}
                            </Button>
                        </CardHeader>
                        <CardContent>
                            {!appInfo ? (
                                <Loading />
                            ) : (
                                <div className="grid gap-3 sm:grid-cols-3 lg:grid-cols-5">
                                    <div className="rounded-lg border p-3">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("env")}
                                        </div>
                                        <div className="mt-1 font-medium">
                                            {appInfo.env}
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-3">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("uptime")}
                                        </div>
                                        <div className="mt-1 font-medium">
                                            {appInfo.uptime}
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-3">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("platform")}
                                        </div>
                                        <div className="mt-1 font-medium">
                                            {appInfo.os} / {appInfo.arch}
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-3">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("hostname")}
                                        </div>
                                        <div className="mt-1 truncate font-medium">
                                            {appInfo.hostname}
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-3">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("commitId")}
                                        </div>
                                        <div className="mt-1 font-mono text-sm">
                                            {appInfo.commit_id || "-"}
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-3">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("cpuUsage")}
                                        </div>
                                        <div className="mt-1 font-medium">
                                            {appInfo.cpu_usage}%
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-3">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("memoryUsage")}
                                        </div>
                                        <div className="mt-1 font-medium">
                                            {appInfo.memory_usage_mb} MB
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-3">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("openFiles")}
                                        </div>
                                        <div className="mt-1 font-medium">
                                            {appInfo.open_files}
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-3 sm:col-span-2">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("diskIo")}
                                        </div>
                                        <div className="mt-1 font-medium">
                                            {appInfo.total_read_mb} MB /{" "}
                                            {appInfo.total_written_mb} MB
                                        </div>
                                    </div>
                                </div>
                            )}
                        </CardContent>
                    </Card>

                    <Card>
                        <CardHeader className="flex flex-row items-center justify-between gap-2">
                            <CardTitle className="flex items-center gap-2">
                                <ActivityIcon className="size-5" />
                                {homeI18n("jobStatsTitle")}
                            </CardTitle>
                            {jobSupported && (
                                <Button variant="outline" size="sm" asChild>
                                    <Link to={JOBS}>
                                        {homeI18n("manageJobs")}
                                    </Link>
                                </Button>
                            )}
                        </CardHeader>
                        <CardContent>
                            {!jobSupported ? (
                                <p className="text-sm text-muted-foreground">
                                    {homeI18n("jobDisabled")}
                                </p>
                            ) : !jobStats ? (
                                <Loading />
                            ) : (
                                <div className="grid gap-4 sm:grid-cols-3">
                                    <div className="rounded-lg border p-4">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("jobPending")}
                                        </div>
                                        <div className="mt-1 text-2xl font-semibold">
                                            {jobStats.pending}
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-4">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("jobRunning")}
                                        </div>
                                        <div className="mt-1 text-2xl font-semibold">
                                            {jobStats.running}
                                        </div>
                                    </div>
                                    <div className="rounded-lg border p-4">
                                        <div className="text-xs text-muted-foreground">
                                            {homeI18n("jobDead")}
                                        </div>
                                        <div className="mt-1 text-2xl font-semibold text-destructive">
                                            {jobStats.dead}
                                        </div>
                                    </div>
                                </div>
                            )}
                        </CardContent>
                    </Card>
                </>
            )}
        </div>
    );
}
