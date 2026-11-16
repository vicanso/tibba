import { useCallback, useEffect, useState } from "react";
import dayjs from "dayjs";
import {
    ArrowLeftIcon,
    CheckIcon,
    CopyIcon,
    DicesIcon,
    EyeIcon,
    KeyRoundIcon,
    PencilIcon,
    PlusIcon,
    RefreshCwIcon,
    RotateCwIcon,
    TrashIcon,
    XIcon,
} from "lucide-react";
import { useSearchParams } from "react-router";
import { toast } from "sonner";
import { useShallow } from "zustand/react/shallow";
import { DateTimePicker } from "@/components/date-time-picker";
import { Loading } from "@/components/loading";
import { StatusBadge } from "@/components/model-components";
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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import { APPS } from "@/constants/route";
import { APPS_URL, SQL_QUERIES_GROUPS, SQL_QUERIES_URL } from "@/constants/url";
import HTTPError from "@/helpers/http-error";
import request from "@/helpers/request";
import { formatDate, formatError, isAdminRole } from "@/helpers/util";
import { useI18n } from "@/i18n";
import useBasicState from "@/states/basic";
import useUserState from "@/states/user";

interface AppClientAdminItem {
    id: number;
    status: number;
    app_id: string;
    description: string;
    secret_prefix: string;
    queries: string[];
    rate_limit_per_min: number;
    effective_start_time: string;
    effective_end_time: string;
    created: string;
    modified: string;
}

interface CreateAppResp {
    id: number;
    app_id: string;
    app_secret: string;
    secret_prefix: string;
}

interface AppSecretResp {
    app_id: string;
    app_secret: string;
    secret_prefix: string;
}

interface SqlQueryOptionItem {
    id: number;
    status: number;
    group: string;
    name: string;
    description: string;
}

interface SqlQueryGroupOptionItem {
    id: number;
    name: string;
    description: string;
}

interface AppDraft {
    appId: string;
    description: string;
    enabled: boolean;
    effectiveStartTime: string;
    effectiveEndTime: string;
    rateLimitPerMin: string;
    queries: string[];
}

interface SecretReveal {
    appId: string;
    appSecret: string;
    secretPrefix: string;
    queries?: string[];
}

const DEFAULT_RATE_LIMIT = 600;

function createEmptyDraft(): AppDraft {
    return {
        appId: "",
        description: "",
        enabled: true,
        effectiveStartTime: "",
        effectiveEndTime: "",
        rateLimitPerMin: String(DEFAULT_RATE_LIMIT),
        queries: [],
    };
}

function itemToDraft(item: AppClientAdminItem): AppDraft {
    return {
        appId: item.app_id,
        description: item.description,
        enabled: item.status === 1,
        effectiveStartTime: item.effective_start_time,
        effectiveEndTime: item.effective_end_time,
        rateLimitPerMin: String(item.rate_limit_per_min),
        queries: item.queries ?? [],
    };
}

function generateRandomAppId(): string {
    const bytes = new Uint8Array(4);
    crypto.getRandomValues(bytes);
    const suffix = Array.from(bytes, (b) =>
        b.toString(16).padStart(2, "0"),
    ).join("");
    return `app_${suffix}`;
}

function resolveCurlTarget(queries?: string[]): string {
    if (!queries || queries.length === 0) {
        return "{group}/{name}";
    }
    const exact = queries.find((q) => !q.endsWith("/*"));
    if (exact) {
        return exact;
    }
    const first = queries[0];
    return first.endsWith("/*") ? `${first.slice(0, -2)}/{name}` : first;
}

function buildCurlExample(
    appId: string,
    appSecret: string,
    queries?: string[],
): string {
    const origin =
        typeof window !== "undefined"
            ? window.location.origin
            : "http://127.0.0.1:8000";
    const target = resolveCurlTarget(queries);
    return [
        `APP_ID="${appId}"`,
        `APP_SECRET="${appSecret}"`,
        `TS=$(date +%s)`,
        `SIGN=$(printf '%s%s' "$APP_SECRET" "$TS" | shasum -a 256 | awk '{print $1}')`,
        `curl "${origin}/api/queries/${target}?app_id=\${APP_ID}&ts=\${TS}&sign=\${SIGN}"`,
    ].join("\n");
}

function parseRateLimit(raw: string): number {
    const num = Number.parseInt(raw.trim(), 10);
    return Number.isFinite(num) && num >= 0 ? num : DEFAULT_RATE_LIMIT;
}

async function copyTextWithToast(
    text: string,
    successMsg: string,
    failMsg: string,
) {
    try {
        await navigator.clipboard.writeText(text);
        toast.success(successMsg);
    } catch {
        toast.error(failMsg);
    }
}

interface SecretRevealCardProps {
    reveal: SecretReveal;
    appsI18n: (key: string) => string;
    onDismiss?: () => void;
}

function SecretRevealCard({
    reveal,
    appsI18n,
    onDismiss,
}: SecretRevealCardProps) {
    const curlExample = buildCurlExample(
        reveal.appId,
        reveal.appSecret,
        reveal.queries,
    );
    return (
        <Card className="border-amber-500/50 bg-amber-500/5">
            <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
                <div>
                    <CardTitle className="text-base">
                        {appsI18n("secretOnceTitle")} (
                        <code className="font-mono">{reveal.appId}</code>)
                    </CardTitle>
                    <p className="mt-1 text-xs text-muted-foreground">
                        {appsI18n("secretOnceHint")}
                    </p>
                </div>
                {onDismiss && (
                    <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={onDismiss}
                    >
                        {appsI18n("dismiss")}
                    </Button>
                )}
            </CardHeader>
            <CardContent className="space-y-3">
                <div className="flex flex-wrap items-center gap-2">
                    <code className="flex-1 overflow-x-auto rounded bg-muted px-3 py-2 font-mono text-xs">
                        {reveal.appSecret}
                    </code>
                    <Button
                        type="button"
                        size="sm"
                        onClick={() =>
                            void copyTextWithToast(
                                reveal.appSecret,
                                appsI18n("secretCopied"),
                                appsI18n("copyFailed"),
                            )
                        }
                    >
                        <CopyIcon className="mr-1.5 size-3.5" />
                        {appsI18n("copySecret")}
                    </Button>
                </div>

                <div className="space-y-1.5">
                    <div className="flex items-center justify-between">
                        <Label className="text-xs">
                            {appsI18n("curlExampleLabel")}
                        </Label>
                        <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            className="h-7 text-xs"
                            onClick={() =>
                                void copyTextWithToast(
                                    curlExample,
                                    appsI18n("curlCopied"),
                                    appsI18n("copyFailed"),
                                )
                            }
                        >
                            <CopyIcon className="mr-1 size-3" />
                            {appsI18n("copyCurl")}
                        </Button>
                    </div>
                    <pre className="overflow-x-auto rounded-md bg-muted p-3 text-xs">
                        {curlExample}
                    </pre>
                </div>
            </CardContent>
        </Card>
    );
}

interface AppEditorViewProps {
    editingItem: AppClientAdminItem | null;
    availableQueries: SqlQueryOptionItem[];
    availableGroups: SqlQueryGroupOptionItem[];
    appsI18n: (key: string) => string;
    componentI18n: (key: string) => string;
    onCancel: () => void;
    onSaved: (reveal: SecretReveal | null) => Promise<void>;
}

function AppEditorView({
    editingItem,
    availableQueries,
    availableGroups,
    appsI18n,
    componentI18n,
    onCancel,
    onSaved,
}: AppEditorViewProps) {
    const [draft, setDraft] = useState<AppDraft>(() =>
        editingItem ? itemToDraft(editingItem) : createEmptyDraft(),
    );
    const [querySearch, setQuerySearch] = useState("");
    const [saving, setSaving] = useState(false);
    const [inlineReveal, setInlineReveal] = useState<SecretReveal | null>(null);
    const [loadingSecret, setLoadingSecret] = useState(false);

    const handleLoadSecret = async () => {
        if (!editingItem) return;
        setLoadingSecret(true);
        try {
            const res = await request.get<AppSecretResp>(
                `${APPS_URL}/${editingItem.id}/secret`,
            );
            setInlineReveal({
                appId: res.data.app_id,
                appSecret: res.data.app_secret,
                secretPrefix: res.data.secret_prefix,
                queries: editingItem.queries,
            });
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setLoadingSecret(false);
        }
    };

    const groupDescMap = new Map<string, string>();
    const groupQueriesMap = new Map<string, SqlQueryOptionItem[]>();
    for (const grp of availableGroups) {
        groupDescMap.set(grp.name, grp.description);
        if (!groupQueriesMap.has(grp.name)) {
            groupQueriesMap.set(grp.name, []);
        }
    }
    for (const q of availableQueries) {
        const list = groupQueriesMap.get(q.group);
        if (list) {
            list.push(q);
        } else {
            groupQueriesMap.set(q.group, [q]);
        }
    }

    const term = querySearch.trim().toLowerCase();
    const visibleGroups = Array.from(groupQueriesMap.entries())
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([groupName, queries]) => {
            const sortedQueries = [...queries].sort((a, b) =>
                a.name.localeCompare(b.name),
            );
            const groupDesc = groupDescMap.get(groupName) ?? "";
            const groupMatches =
                !term ||
                groupName.toLowerCase().includes(term) ||
                groupDesc.toLowerCase().includes(term);
            const filteredQueries = groupMatches
                ? sortedQueries
                : sortedQueries.filter(
                      (q) =>
                          q.name.toLowerCase().includes(term) ||
                          q.description.toLowerCase().includes(term) ||
                          `${q.group}/${q.name}`.toLowerCase().includes(term),
                  );
            return {
                groupName,
                groupDesc,
                queries: filteredQueries,
                groupMatches,
            };
        })
        .filter((g) => g.groupMatches || g.queries.length > 0);

    const toggleGroupWildcard = (groupName: string, enabled: boolean) => {
        const wildcard = `${groupName}/*`;
        const prefix = `${groupName}/`;
        setDraft((d) => {
            const next = d.queries.filter((q) =>
                enabled ? !q.startsWith(prefix) : q !== wildcard,
            );
            if (enabled) {
                next.push(wildcard);
            }
            next.sort((a, b) => a.localeCompare(b));
            return { ...d, queries: next };
        });
    };

    const toggleSingleQuery = (groupName: string, queryName: string) => {
        const wildcard = `${groupName}/*`;
        const fullKey = `${groupName}/${queryName}`;
        setDraft((d) => {
            if (d.queries.includes(wildcard)) {
                return d;
            }
            const exists = d.queries.includes(fullKey);
            const next = exists
                ? d.queries.filter((q) => q !== fullKey)
                : [...d.queries, fullKey].sort((a, b) => a.localeCompare(b));
            return { ...d, queries: next };
        });
    };

    const removeGrant = (grant: string) => {
        setDraft((d) => ({
            ...d,
            queries: d.queries.filter((q) => q !== grant),
        }));
    };

    const handleSave = async () => {
        if (!editingItem && !draft.appId.trim()) {
            toast.error(appsI18n("appIdRequired"));
            return;
        }
        setSaving(true);
        try {
            const status = draft.enabled ? 1 : 0;
            const rateLimitPerMin = parseRateLimit(draft.rateLimitPerMin);
            if (editingItem) {
                const payload: Record<string, unknown> = {
                    description: draft.description,
                    status,
                    rate_limit_per_min: rateLimitPerMin,
                    queries: draft.queries,
                };
                if (draft.effectiveStartTime.trim()) {
                    payload.effective_start_time =
                        draft.effectiveStartTime.trim();
                }
                if (draft.effectiveEndTime.trim()) {
                    payload.effective_end_time = draft.effectiveEndTime.trim();
                }
                await request.patch(`${APPS_URL}/${editingItem.id}`, payload);
                toast.success(appsI18n("updateSuccess"));
                await onSaved(null);
            } else {
                const payload: Record<string, unknown> = {
                    app_id: draft.appId.trim(),
                    description: draft.description,
                    status,
                    rate_limit_per_min: rateLimitPerMin,
                    queries: draft.queries,
                };
                if (draft.effectiveStartTime.trim()) {
                    payload.effective_start_time =
                        draft.effectiveStartTime.trim();
                }
                if (draft.effectiveEndTime.trim()) {
                    payload.effective_end_time = draft.effectiveEndTime.trim();
                }
                const res = await request.post<CreateAppResp>(
                    APPS_URL,
                    payload,
                );
                toast.success(appsI18n("createSuccess"));
                await onSaved({
                    appId: res.data.app_id,
                    appSecret: res.data.app_secret,
                    secretPrefix: res.data.secret_prefix,
                    queries: draft.queries,
                });
            }
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setSaving(false);
        }
    };

    return (
        <div className="w-full space-y-6 p-2 md:p-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
                <div className="flex flex-wrap items-center gap-3">
                    <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={onCancel}
                    >
                        <ArrowLeftIcon className="mr-1.5 size-4" />
                        {appsI18n("backToList")}
                    </Button>
                    <h1 className="text-xl font-semibold">
                        {editingItem
                            ? appsI18n("editTitle").replace(
                                  "{appId}",
                                  editingItem.app_id,
                              )
                            : appsI18n("createTitle")}
                    </h1>
                </div>
                <div className="flex items-center gap-2">
                    <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={onCancel}
                    >
                        {appsI18n("cancel")}
                    </Button>
                    <Button
                        type="button"
                        size="sm"
                        disabled={saving}
                        onClick={() => void handleSave()}
                    >
                        {saving ? appsI18n("saving") : appsI18n("save")}
                    </Button>
                </div>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle className="text-base">
                        {appsI18n("basicSection")}
                    </CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="grid gap-3 md:grid-cols-2">
                        <div className="space-y-1.5">
                            <Label>{appsI18n("colAppId")}</Label>
                            <div className="flex gap-2">
                                <Input
                                    className="font-mono"
                                    disabled={Boolean(editingItem)}
                                    value={draft.appId}
                                    placeholder={appsI18n("appIdPlaceholder")}
                                    onChange={(e) =>
                                        setDraft((d) => ({
                                            ...d,
                                            appId: e.target.value,
                                        }))
                                    }
                                />
                                {!editingItem && (
                                    <Button
                                        type="button"
                                        variant="outline"
                                        size="sm"
                                        className="shrink-0"
                                        onClick={() =>
                                            setDraft((d) => ({
                                                ...d,
                                                appId: generateRandomAppId(),
                                            }))
                                        }
                                    >
                                        <DicesIcon className="mr-1.5 size-4" />
                                        {appsI18n("generateAppId")}
                                    </Button>
                                )}
                            </div>
                        </div>
                        <div className="space-y-1.5">
                            <Label>{appsI18n("description")}</Label>
                            <Input
                                value={draft.description}
                                placeholder={appsI18n("descriptionPlaceholder")}
                                onChange={(e) =>
                                    setDraft((d) => ({
                                        ...d,
                                        description: e.target.value,
                                    }))
                                }
                            />
                        </div>
                    </div>

                    <div className="grid items-end gap-3 md:grid-cols-4">
                        <div className="flex h-9 items-center gap-2">
                            <Switch
                                checked={draft.enabled}
                                onCheckedChange={(checked) =>
                                    setDraft((d) => ({
                                        ...d,
                                        enabled: checked,
                                    }))
                                }
                            />
                            <Label>{appsI18n("enabled")}</Label>
                        </div>
                        <div className="space-y-1.5">
                            <Label>{appsI18n("rateLimitLabel")}</Label>
                            <Input
                                type="number"
                                min={0}
                                max={600000}
                                value={draft.rateLimitPerMin}
                                onChange={(e) =>
                                    setDraft((d) => ({
                                        ...d,
                                        rateLimitPerMin: e.target.value,
                                    }))
                                }
                            />
                        </div>
                        <div className="space-y-1.5">
                            <Label>{appsI18n("effectiveStartTime")}</Label>
                            <DateTimePicker
                                clearable
                                i18n={componentI18n}
                                date={draft.effectiveStartTime || null}
                                placeholder={appsI18n("timePlaceholder")}
                                setDate={(date) =>
                                    setDraft((d) => ({
                                        ...d,
                                        effectiveStartTime: date
                                            ? dayjs(date).format(
                                                  "YYYY-MM-DDTHH:mm:ssZ",
                                              )
                                            : "",
                                    }))
                                }
                            />
                        </div>
                        <div className="space-y-1.5">
                            <Label>{appsI18n("effectiveEndTime")}</Label>
                            <DateTimePicker
                                clearable
                                i18n={componentI18n}
                                date={draft.effectiveEndTime || null}
                                placeholder={appsI18n("timePlaceholder")}
                                setDate={(date) =>
                                    setDraft((d) => ({
                                        ...d,
                                        effectiveEndTime: date
                                            ? dayjs(date).format(
                                                  "YYYY-MM-DDTHH:mm:ssZ",
                                              )
                                            : "",
                                    }))
                                }
                            />
                        </div>
                    </div>
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle className="text-base">
                        {appsI18n("queriesSection")}
                    </CardTitle>
                    <p className="text-xs text-muted-foreground">
                        {appsI18n("queriesHint")}
                    </p>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="flex flex-wrap items-center gap-1.5 rounded-md border bg-muted/30 p-2.5">
                        {draft.queries.length === 0 ? (
                            <span className="text-xs text-muted-foreground">
                                {appsI18n("noQueriesSelected")}
                            </span>
                        ) : (
                            draft.queries.map((grant) => (
                                <Badge
                                    key={grant}
                                    variant={
                                        grant.endsWith("/*")
                                            ? "default"
                                            : "secondary"
                                    }
                                    className="gap-1 font-mono text-xs"
                                >
                                    <span>{grant}</span>
                                    <button
                                        type="button"
                                        aria-label={grant}
                                        className="rounded-sm hover:opacity-75"
                                        onClick={() => removeGrant(grant)}
                                    >
                                        <XIcon className="size-3" />
                                    </button>
                                </Badge>
                            ))
                        )}
                    </div>

                    <Input
                        value={querySearch}
                        placeholder={appsI18n("querySearchPlaceholder")}
                        onChange={(e) => setQuerySearch(e.target.value)}
                    />

                    {groupQueriesMap.size === 0 ? (
                        <div className="rounded-md border border-dashed p-6 text-center text-xs text-muted-foreground">
                            {appsI18n("noSqlQueriesConfigured")}
                        </div>
                    ) : visibleGroups.length === 0 ? (
                        <div className="rounded-md border border-dashed p-6 text-center text-xs text-muted-foreground">
                            {appsI18n("noMatchingQueries")}
                        </div>
                    ) : (
                        <div className="space-y-3">
                            {visibleGroups.map(
                                ({ groupName, groupDesc, queries }) => {
                                    const wildcard = `${groupName}/*`;
                                    const wildcardActive =
                                        draft.queries.includes(wildcard);
                                    return (
                                        <div
                                            key={groupName}
                                            className="space-y-3 rounded-lg border p-3"
                                        >
                                            <div className="flex flex-wrap items-center justify-between gap-2">
                                                <div className="flex flex-wrap items-center gap-2">
                                                    <Badge
                                                        variant="outline"
                                                        className="font-mono text-xs"
                                                    >
                                                        {groupName}
                                                    </Badge>
                                                    {groupDesc && (
                                                        <span className="text-xs text-muted-foreground">
                                                            {groupDesc}
                                                        </span>
                                                    )}
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <Switch
                                                        checked={wildcardActive}
                                                        onCheckedChange={(
                                                            checked,
                                                        ) =>
                                                            toggleGroupWildcard(
                                                                groupName,
                                                                checked,
                                                            )
                                                        }
                                                    />
                                                    <span className="text-xs">
                                                        {appsI18n(
                                                            "grantWholeGroup",
                                                        )}
                                                    </span>
                                                </div>
                                            </div>

                                            {queries.length === 0 ? (
                                                <div className="text-xs text-muted-foreground">
                                                    {appsI18n(
                                                        "emptyGroupQueries",
                                                    )}
                                                </div>
                                            ) : (
                                                <div className="grid gap-2 md:grid-cols-2">
                                                    {queries.map((q) => {
                                                        const fullKey = `${q.group}/${q.name}`;
                                                        const checked =
                                                            wildcardActive ||
                                                            draft.queries.includes(
                                                                fullKey,
                                                            );
                                                        return (
                                                            <button
                                                                key={fullKey}
                                                                type="button"
                                                                disabled={
                                                                    wildcardActive
                                                                }
                                                                onClick={() =>
                                                                    toggleSingleQuery(
                                                                        q.group,
                                                                        q.name,
                                                                    )
                                                                }
                                                                className={`flex items-start gap-2.5 rounded-md border p-2.5 text-left transition-colors ${
                                                                    checked
                                                                        ? "border-primary/60 bg-primary/5"
                                                                        : "hover:bg-muted/50"
                                                                } ${wildcardActive ? "cursor-not-allowed opacity-70" : ""}`}
                                                            >
                                                                <span
                                                                    className={`mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-sm border ${
                                                                        checked
                                                                            ? "border-primary bg-primary text-primary-foreground"
                                                                            : "border-muted-foreground/40"
                                                                    }`}
                                                                >
                                                                    {checked && (
                                                                        <CheckIcon className="size-3" />
                                                                    )}
                                                                </span>
                                                                <div className="min-w-0 flex-1 space-y-0.5">
                                                                    <div className="flex flex-wrap items-center gap-1.5">
                                                                        <span className="truncate font-mono text-xs font-medium">
                                                                            {
                                                                                fullKey
                                                                            }
                                                                        </span>
                                                                        {q.status !==
                                                                            1 && (
                                                                            <Badge
                                                                                variant="outline"
                                                                                className="px-1 py-0 text-[10px]"
                                                                            >
                                                                                {appsI18n(
                                                                                    "queryDisabledBadge",
                                                                                )}
                                                                            </Badge>
                                                                        )}
                                                                    </div>
                                                                    {q.description && (
                                                                        <div className="truncate text-xs text-muted-foreground">
                                                                            {
                                                                                q.description
                                                                            }
                                                                        </div>
                                                                    )}
                                                                </div>
                                                            </button>
                                                        );
                                                    })}
                                                </div>
                                            )}
                                        </div>
                                    );
                                },
                            )}
                        </div>
                    )}
                </CardContent>
            </Card>

            {editingItem && (
                <div className="space-y-4">
                    <Card>
                        <CardContent className="flex flex-wrap items-center justify-between gap-3 py-4">
                            <div className="space-y-1">
                                <div className="text-sm font-medium">
                                    {appsI18n("secretOnceTitle")}
                                </div>
                                <div className="text-xs text-muted-foreground">
                                    {appsI18n("colSecretPrefix")}:{" "}
                                    <code className="rounded bg-muted px-1.5 py-0.5 font-mono">
                                        {editingItem.secret_prefix}••••••••
                                    </code>
                                </div>
                            </div>
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                disabled={loadingSecret}
                                onClick={() => void handleLoadSecret()}
                            >
                                <EyeIcon className="mr-1.5 size-3.5" />
                                {loadingSecret
                                    ? appsI18n("loadingSecret")
                                    : appsI18n("viewSecret")}
                            </Button>
                        </CardContent>
                    </Card>

                    {inlineReveal && (
                        <SecretRevealCard
                            reveal={inlineReveal}
                            appsI18n={appsI18n}
                            onDismiss={() => setInlineReveal(null)}
                        />
                    )}
                </div>
            )}

            <div className="flex justify-end gap-2">
                <Button type="button" variant="outline" onClick={onCancel}>
                    {appsI18n("cancel")}
                </Button>
                <Button
                    type="button"
                    disabled={saving}
                    onClick={() => void handleSave()}
                >
                    {saving ? appsI18n("saving") : appsI18n("save")}
                </Button>
            </div>
        </div>
    );
}

export default function Apps() {
    const commonI18n = useI18n("common");
    const appsI18n = useI18n("apps");
    const modelI18n = useI18n("model");
    const componentI18n = useI18n("component");
    const [searchParams, setSearchParams] = useSearchParams();
    const [setPageHeaderNavigation, resetPageHeaderNavigation] = useBasicState(
        useShallow((state) => [
            state.setPageHeaderNavigation,
            state.resetPageHeaderNavigation,
        ]),
    );
    const [user, initialized] = useUserState(
        useShallow((state) => [state.data, state.initialized]),
    );

    const canAccess = isAdminRole(user.roles);

    const [items, setItems] = useState<AppClientAdminItem[]>([]);
    const [availableQueries, setAvailableQueries] = useState<
        SqlQueryOptionItem[]
    >([]);
    const [availableGroups, setAvailableGroups] = useState<
        SqlQueryGroupOptionItem[]
    >([]);
    const [loaded, setLoaded] = useState(false);
    const [refreshing, setRefreshing] = useState(false);
    const [featureDisabled, setFeatureDisabled] = useState(false);

    const [secretReveal, setSecretReveal] = useState<SecretReveal | null>(null);
    const [loadingSecretId, setLoadingSecretId] = useState<number | null>(null);
    const [rotateTarget, setRotateTarget] = useState<AppClientAdminItem | null>(
        null,
    );
    const [deleteTarget, setDeleteTarget] = useState<AppClientAdminItem | null>(
        null,
    );

    const mode = searchParams.get("mode");
    const rawId = searchParams.get("id");
    const editId = rawId ? Number(rawId) : Number.NaN;
    const editingTarget =
        mode === "edit" && Number.isFinite(editId)
            ? (items.find((item) => item.id === editId) ?? null)
            : null;

    const applyLoadError = useCallback((err: unknown) => {
        setLoaded(true);
        if (err instanceof HTTPError && err.status === 404) {
            setFeatureDisabled(true);
        } else {
            toast.error(formatError(err));
        }
    }, []);

    const refreshData = useCallback(async () => {
        if (!canAccess) return;
        setRefreshing(true);
        try {
            const [appsRes, queriesRes, groupsRes] = await Promise.all([
                request.get<AppClientAdminItem[]>(APPS_URL),
                request
                    .get<SqlQueryOptionItem[]>(SQL_QUERIES_URL)
                    .catch(() => ({ data: [] })),
                request
                    .get<SqlQueryGroupOptionItem[]>(SQL_QUERIES_GROUPS)
                    .catch(() => ({ data: [] })),
            ]);
            setItems(appsRes.data);
            setAvailableQueries(queriesRes.data);
            setAvailableGroups(groupsRes.data);
            setFeatureDisabled(false);
            setLoaded(true);
        } catch (err) {
            applyLoadError(err);
        } finally {
            setRefreshing(false);
        }
    }, [canAccess, applyLoadError]);

    useEffect(() => {
        if (mode === "create") {
            setPageHeaderNavigation([
                {
                    title: appsI18n("title"),
                    icon: <KeyRoundIcon className="size-4" />,
                    url: APPS,
                },
                {
                    title: appsI18n("createTitle"),
                },
            ]);
        } else if (mode === "edit") {
            setPageHeaderNavigation([
                {
                    title: appsI18n("title"),
                    icon: <KeyRoundIcon className="size-4" />,
                    url: APPS,
                },
                {
                    title: editingTarget
                        ? appsI18n("editTitle").replace(
                              "{appId}",
                              editingTarget.app_id,
                          )
                        : appsI18n("edit"),
                },
            ]);
        } else {
            setPageHeaderNavigation([
                {
                    title: appsI18n("title"),
                    icon: <KeyRoundIcon className="size-4" />,
                },
            ]);
        }
        return () => {
            resetPageHeaderNavigation();
        };
    }, [
        mode,
        editingTarget,
        appsI18n,
        setPageHeaderNavigation,
        resetPageHeaderNavigation,
    ]);

    useEffect(() => {
        if (!initialized || !canAccess) return;
        let cancelled = false;
        Promise.all([
            request.get<AppClientAdminItem[]>(APPS_URL),
            request
                .get<SqlQueryOptionItem[]>(SQL_QUERIES_URL)
                .catch(() => ({ data: [] })),
            request
                .get<SqlQueryGroupOptionItem[]>(SQL_QUERIES_GROUPS)
                .catch(() => ({ data: [] })),
        ])
            .then(([appsRes, queriesRes, groupsRes]) => {
                if (!cancelled) {
                    setItems(appsRes.data);
                    setAvailableQueries(queriesRes.data);
                    setAvailableGroups(groupsRes.data);
                    setFeatureDisabled(false);
                    setLoaded(true);
                }
            })
            .catch((err) => {
                if (!cancelled) {
                    applyLoadError(err);
                }
            });
        return () => {
            cancelled = true;
        };
    }, [initialized, canAccess, applyLoadError]);

    const backToList = () => {
        setSearchParams({});
    };

    const handleSaved = async (reveal: SecretReveal | null) => {
        if (reveal) {
            setSecretReveal(reveal);
        }
        await refreshData();
        setSearchParams({});
    };

    const handleRotateConfirm = async () => {
        if (!rotateTarget) return;
        const target = rotateTarget;
        setRotateTarget(null);
        try {
            const res = await request.post<AppSecretResp>(
                `${APPS_URL}/${target.id}/rotate-secret`,
            );
            setSecretReveal({
                appId: res.data.app_id,
                appSecret: res.data.app_secret,
                secretPrefix: res.data.secret_prefix,
                queries: target.queries,
            });
            toast.success(appsI18n("rotateSuccess"));
            await refreshData();
        } catch (err) {
            toast.error(formatError(err));
        }
    };

    const handleViewSecret = async (item: AppClientAdminItem) => {
        setLoadingSecretId(item.id);
        try {
            const res = await request.get<AppSecretResp>(
                `${APPS_URL}/${item.id}/secret`,
            );
            setSecretReveal({
                appId: res.data.app_id,
                appSecret: res.data.app_secret,
                secretPrefix: res.data.secret_prefix,
                queries: item.queries,
            });
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setLoadingSecretId(null);
        }
    };

    const handleDeleteConfirm = async () => {
        if (!deleteTarget) return;
        const target = deleteTarget;
        setDeleteTarget(null);
        try {
            await request.delete(`${APPS_URL}/${target.id}`);
            if (secretReveal?.appId === target.app_id) {
                setSecretReveal(null);
            }
            toast.success(appsI18n("deleteSuccess"));
            await refreshData();
        } catch (err) {
            toast.error(formatError(err));
        }
    };

    if (!initialized) {
        return <Loading />;
    }

    if (!canAccess) {
        return (
            <div className="p-6 text-center text-muted-foreground">
                {commonI18n("noPermission")}
            </div>
        );
    }

    if (featureDisabled) {
        return (
            <div className="p-6 text-center text-muted-foreground">
                {appsI18n("featureDisabled")}
            </div>
        );
    }

    if (mode === "create") {
        return (
            <AppEditorView
                key="create"
                editingItem={null}
                availableQueries={availableQueries}
                availableGroups={availableGroups}
                appsI18n={appsI18n}
                componentI18n={componentI18n}
                onCancel={backToList}
                onSaved={handleSaved}
            />
        );
    }

    if (mode === "edit") {
        if (!loaded) {
            return <Loading />;
        }
        if (!editingTarget) {
            return (
                <div className="w-full space-y-6 p-2 md:p-4">
                    <Card>
                        <CardContent className="flex flex-col items-center gap-4 py-12 text-center">
                            <p className="text-sm text-muted-foreground">
                                {appsI18n("notFound")}
                            </p>
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                onClick={backToList}
                            >
                                <ArrowLeftIcon className="mr-1.5 size-4" />
                                {appsI18n("backToList")}
                            </Button>
                        </CardContent>
                    </Card>
                </div>
            );
        }
        return (
            <AppEditorView
                key={`edit-${editingTarget.id}`}
                editingItem={editingTarget}
                availableQueries={availableQueries}
                availableGroups={availableGroups}
                appsI18n={appsI18n}
                componentI18n={componentI18n}
                onCancel={backToList}
                onSaved={handleSaved}
            />
        );
    }

    const loading = refreshing || !loaded;

    return (
        <div className="w-full space-y-6 p-2 md:p-4">
            <div className="flex flex-wrap items-center justify-between gap-2">
                <div>
                    <h1 className="text-xl font-semibold">
                        {appsI18n("title")}
                    </h1>
                    <p className="mt-1 text-sm text-muted-foreground">
                        {appsI18n("descriptionHint")}
                    </p>
                </div>
                <div className="flex items-center gap-2">
                    <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        disabled={loading}
                        onClick={() => void refreshData()}
                    >
                        <RefreshCwIcon
                            className={`mr-1.5 size-4 ${loading ? "animate-spin" : ""}`}
                        />
                        {appsI18n("refresh")}
                    </Button>
                    <Button
                        type="button"
                        size="sm"
                        onClick={() => setSearchParams({ mode: "create" })}
                    >
                        <PlusIcon className="mr-1.5 size-4" />
                        {appsI18n("create")}
                    </Button>
                </div>
            </div>

            {secretReveal && (
                <SecretRevealCard
                    reveal={secretReveal}
                    appsI18n={appsI18n}
                    onDismiss={() => setSecretReveal(null)}
                />
            )}

            <Card>
                <CardHeader>
                    <CardTitle className="text-base">
                        {appsI18n("title")} ({items.length})
                    </CardTitle>
                </CardHeader>
                <CardContent>
                    {loading && items.length === 0 ? (
                        <Loading />
                    ) : items.length === 0 ? (
                        <div className="py-8 text-center text-sm text-muted-foreground">
                            {appsI18n("empty")}
                        </div>
                    ) : (
                        <div className="overflow-x-auto">
                            <Table>
                                <TableHeader>
                                    <TableRow>
                                        <TableHead>
                                            {appsI18n("colAppId")}
                                        </TableHead>
                                        <TableHead>
                                            {appsI18n("colSecretPrefix")}
                                        </TableHead>
                                        <TableHead>
                                            {appsI18n("colQueries")}
                                        </TableHead>
                                        <TableHead>
                                            {appsI18n("colRateLimit")}
                                        </TableHead>
                                        <TableHead>
                                            {appsI18n("colStatus")}
                                        </TableHead>
                                        <TableHead>
                                            {appsI18n("colModified")}
                                        </TableHead>
                                        <TableHead className="text-right">
                                            {appsI18n("colAction")}
                                        </TableHead>
                                    </TableRow>
                                </TableHeader>
                                <TableBody>
                                    {items.map((item) => (
                                        <TableRow key={item.id}>
                                            <TableCell className="align-top">
                                                <div className="font-mono text-sm font-medium">
                                                    {item.app_id}
                                                </div>
                                                {item.description && (
                                                    <div className="max-w-xs truncate text-xs text-muted-foreground">
                                                        {item.description}
                                                    </div>
                                                )}
                                            </TableCell>
                                            <TableCell className="align-top">
                                                <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">
                                                    {item.secret_prefix}••••••••
                                                </code>
                                            </TableCell>
                                            <TableCell className="align-top">
                                                {(item.queries?.length ?? 0) ===
                                                0 ? (
                                                    <span className="text-xs text-muted-foreground">
                                                        {appsI18n(
                                                            "noQueriesSelected",
                                                        )}
                                                    </span>
                                                ) : (
                                                    <div className="flex max-w-md flex-wrap gap-1">
                                                        {item.queries.map(
                                                            (query) => (
                                                                <Badge
                                                                    key={query}
                                                                    variant={
                                                                        query.endsWith(
                                                                            "/*",
                                                                        )
                                                                            ? "default"
                                                                            : "secondary"
                                                                    }
                                                                    className="font-mono text-xs"
                                                                >
                                                                    {query}
                                                                </Badge>
                                                            ),
                                                        )}
                                                    </div>
                                                )}
                                            </TableCell>
                                            <TableCell className="align-top text-xs">
                                                {item.rate_limit_per_min > 0
                                                    ? appsI18n(
                                                          "rateLimitPerMin",
                                                      ).replace(
                                                          "{limit}",
                                                          String(
                                                              item.rate_limit_per_min,
                                                          ),
                                                      )
                                                    : appsI18n("rateUnlimited")}
                                            </TableCell>
                                            <TableCell className="align-top">
                                                <StatusBadge
                                                    status={String(item.status)}
                                                    i18nModel={modelI18n}
                                                />
                                            </TableCell>
                                            <TableCell className="align-top text-xs text-muted-foreground">
                                                {formatDate(item.modified)}
                                            </TableCell>
                                            <TableCell className="align-top text-right">
                                                <div className="flex justify-end gap-1">
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        size="sm"
                                                        disabled={
                                                            loadingSecretId ===
                                                            item.id
                                                        }
                                                        onClick={() =>
                                                            void handleViewSecret(
                                                                item,
                                                            )
                                                        }
                                                    >
                                                        <EyeIcon className="mr-1 size-3.5" />
                                                        {loadingSecretId ===
                                                        item.id
                                                            ? appsI18n(
                                                                  "loadingSecret",
                                                              )
                                                            : appsI18n(
                                                                  "viewSecret",
                                                              )}
                                                    </Button>
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        size="sm"
                                                        onClick={() =>
                                                            setSearchParams({
                                                                mode: "edit",
                                                                id: String(
                                                                    item.id,
                                                                ),
                                                            })
                                                        }
                                                    >
                                                        <PencilIcon className="mr-1 size-3.5" />
                                                        {appsI18n("edit")}
                                                    </Button>
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        size="sm"
                                                        onClick={() =>
                                                            setRotateTarget(
                                                                item,
                                                            )
                                                        }
                                                    >
                                                        <RotateCwIcon className="mr-1 size-3.5" />
                                                        {appsI18n(
                                                            "rotateSecret",
                                                        )}
                                                    </Button>
                                                    <Button
                                                        type="button"
                                                        variant="ghost"
                                                        size="icon"
                                                        className="size-8 text-destructive hover:text-destructive"
                                                        onClick={() =>
                                                            setDeleteTarget(
                                                                item,
                                                            )
                                                        }
                                                    >
                                                        <TrashIcon className="size-4" />
                                                    </Button>
                                                </div>
                                            </TableCell>
                                        </TableRow>
                                    ))}
                                </TableBody>
                            </Table>
                        </div>
                    )}
                </CardContent>
            </Card>

            <AlertDialog
                open={Boolean(rotateTarget)}
                onOpenChange={(open) => {
                    if (!open) setRotateTarget(null);
                }}
            >
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>
                            {appsI18n("rotateTitle")}
                        </AlertDialogTitle>
                        <AlertDialogDescription>
                            {appsI18n("rotateConfirm").replace(
                                "{appId}",
                                rotateTarget?.app_id ?? "",
                            )}
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>
                            {appsI18n("cancel")}
                        </AlertDialogCancel>
                        <AlertDialogAction
                            onClick={() => void handleRotateConfirm()}
                        >
                            {appsI18n("rotateSecret")}
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>

            <AlertDialog
                open={Boolean(deleteTarget)}
                onOpenChange={(open) => {
                    if (!open) setDeleteTarget(null);
                }}
            >
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>
                            {appsI18n("deleteTitle")}
                        </AlertDialogTitle>
                        <AlertDialogDescription>
                            {appsI18n("deleteConfirm").replace(
                                "{appId}",
                                deleteTarget?.app_id ?? "",
                            )}
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>
                            {appsI18n("cancel")}
                        </AlertDialogCancel>
                        <AlertDialogAction
                            onClick={() => void handleDeleteConfirm()}
                        >
                            {appsI18n("deleteTitle")}
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </div>
    );
}
