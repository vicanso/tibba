import { useCallback, useEffect, useState } from "react";
import dayjs from "dayjs";
import {
    ArrowDownIcon,
    ArrowLeftIcon,
    ArrowUpIcon,
    CopyIcon,
    DatabaseIcon,
    PencilIcon,
    PlayIcon,
    PlusIcon,
    RefreshCwIcon,
    TrashIcon,
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
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { SQL_QUERIES } from "@/constants/route";
import {
    SQL_QUERIES_GROUPS,
    SQL_QUERIES_TEST,
    SQL_QUERIES_URL,
} from "@/constants/url";
import HTTPError from "@/helpers/http-error";
import request from "@/helpers/request";
import {
    formatDate,
    formatError,
    isAdminRole,
    isSuperAdminRole,
} from "@/helpers/util";
import { useI18n } from "@/i18n";
import useBasicState from "@/states/basic";
import useUserState from "@/states/user";

type SqlAccessLevel = "admin" | "permission" | "login" | "public";
type SqlResultMode = "list" | "single";
type SqlParamType =
    | "int"
    | "float"
    | "bool"
    | "string"
    | "datetime"
    | "current_user_id"
    | "current_account";

interface SqlParamDef {
    name: string;
    param_type: SqlParamType;
    required: boolean;
    default_value?: string | null;
    min?: number | null;
    max?: number | null;
}

interface SqlQuerySpec {
    sql: string;
    params: SqlParamDef[];
    access: SqlAccessLevel;
    permission_code: string;
    result_mode: SqlResultMode;
    timeout_ms: number;
    max_rows: number;
    cache_ttl_secs: number;
}

interface SqlQueryAdminItem {
    id: number;
    status: number;
    group: string;
    name: string;
    description: string;
    effective_start_time: string;
    effective_end_time: string;
    created: string;
    modified: string;
    spec: SqlQuerySpec | null;
    raw_data: Record<string, unknown>;
}

interface SqlQueryGroupItem {
    id: number;
    name: string;
    description: string;
    query_count: number;
    created: string;
    modified: string;
}

interface TestSqlQueryResp {
    elapsed_ms: number;
    row_count: number;
    result: unknown;
}

interface ParamDraft {
    name: string;
    param_type: SqlParamType;
    required: boolean;
    default_value: string;
    min: string;
    max: string;
}

interface QueryDraft {
    group: string;
    name: string;
    description: string;
    enabled: boolean;
    effectiveStartTime: string;
    effectiveEndTime: string;
    sql: string;
    params: ParamDraft[];
    access: SqlAccessLevel;
    permissionCode: string;
    resultMode: SqlResultMode;
    timeoutMs: string;
    maxRows: string;
    cacheTtlSecs: string;
}

const DEFAULT_TIMEOUT_MS = 3000;
const DEFAULT_MAX_ROWS = 200;
const DEFAULT_CACHE_TTL_SECS = 0;

function isSessionParamType(paramType: SqlParamType): boolean {
    return paramType === "current_user_id" || paramType === "current_account";
}

function isNumericParamType(paramType: SqlParamType): boolean {
    return paramType === "int" || paramType === "float";
}

function createEmptyDraft(): QueryDraft {
    return {
        group: "",
        name: "",
        description: "",
        enabled: true,
        effectiveStartTime: "",
        effectiveEndTime: "",
        sql: "",
        params: [],
        access: "admin",
        permissionCode: "",
        resultMode: "list",
        timeoutMs: String(DEFAULT_TIMEOUT_MS),
        maxRows: String(DEFAULT_MAX_ROWS),
        cacheTtlSecs: String(DEFAULT_CACHE_TTL_SECS),
    };
}

function createEmptyParam(): ParamDraft {
    return {
        name: "",
        param_type: "string",
        required: true,
        default_value: "",
        min: "",
        max: "",
    };
}

function itemToDraft(item: SqlQueryAdminItem): QueryDraft {
    const spec = item.spec;
    return {
        group: item.group,
        name: item.name,
        description: item.description,
        enabled: item.status === 1,
        effectiveStartTime: item.effective_start_time,
        effectiveEndTime: item.effective_end_time,
        sql:
            spec?.sql ??
            (typeof item.raw_data.sql === "string" ? item.raw_data.sql : ""),
        params: (spec?.params ?? []).map((p) => ({
            name: p.name,
            param_type: p.param_type,
            required: p.required,
            default_value: p.default_value ?? "",
            min: p.min != null ? String(p.min) : "",
            max: p.max != null ? String(p.max) : "",
        })),
        access: spec?.access ?? "admin",
        permissionCode: spec?.permission_code ?? "",
        resultMode: spec?.result_mode ?? "list",
        timeoutMs: String(spec?.timeout_ms ?? DEFAULT_TIMEOUT_MS),
        maxRows: String(spec?.max_rows ?? DEFAULT_MAX_ROWS),
        cacheTtlSecs: String(spec?.cache_ttl_secs ?? DEFAULT_CACHE_TTL_SECS),
    };
}

function buildInitialTestArgs(
    item: SqlQueryAdminItem | null,
): Record<string, string> {
    if (!item?.spec) return {};
    const initialArgs: Record<string, string> = {};
    for (const p of item.spec.params) {
        if (!isSessionParamType(p.param_type) && p.name.trim()) {
            initialArgs[p.name.trim()] = p.default_value ?? "";
        }
    }
    return initialArgs;
}

function parseOptionalNumber(raw: string): number | null {
    const trimmed = raw.trim();
    if (!trimmed) return null;
    const num = Number(trimmed);
    return Number.isFinite(num) ? num : null;
}

function parseIntegerOrDefault(raw: string, fallback: number): number {
    const num = Number.parseInt(raw.trim(), 10);
    return Number.isFinite(num) ? num : fallback;
}

function buildSpecFromDraft(draft: QueryDraft): SqlQuerySpec {
    return {
        sql: draft.sql,
        params: draft.params.map((p) => {
            const sessionBound = isSessionParamType(p.param_type);
            const numeric = isNumericParamType(p.param_type);
            return {
                name: p.name.trim(),
                param_type: p.param_type,
                required: sessionBound ? false : p.required,
                default_value:
                    sessionBound || !p.default_value.trim()
                        ? null
                        : p.default_value,
                min: numeric ? parseOptionalNumber(p.min) : null,
                max: numeric ? parseOptionalNumber(p.max) : null,
            };
        }),
        access: draft.access,
        permission_code:
            draft.access === "permission" ? draft.permissionCode.trim() : "",
        result_mode: draft.resultMode,
        timeout_ms: parseIntegerOrDefault(draft.timeoutMs, DEFAULT_TIMEOUT_MS),
        max_rows: parseIntegerOrDefault(draft.maxRows, DEFAULT_MAX_ROWS),
        cache_ttl_secs: parseIntegerOrDefault(
            draft.cacheTtlSecs,
            DEFAULT_CACHE_TTL_SECS,
        ),
    };
}

function endpointPath(group: string, name: string): string {
    return `/api/queries/${group}/${name}`;
}

function formatAccessLabel(
    access: SqlAccessLevel,
    permissionCode: string,
    t: (k: string) => string,
): string {
    switch (access) {
        case "admin":
            return t("accessAdmin");
        case "permission":
            return permissionCode
                ? `perm: ${permissionCode}`
                : t("accessPermission");
        case "login":
            return t("accessLogin");
        case "public":
            return t("accessPublic");
    }
}

interface ParamEditorProps {
    params: ParamDraft[];
    onChange: (next: ParamDraft[]) => void;
    t: (key: string) => string;
}

function ParamBuilder({ params, onChange, t }: ParamEditorProps) {
    const updateAt = (index: number, patch: Partial<ParamDraft>) => {
        onChange(
            params.map((item, i) =>
                i === index ? { ...item, ...patch } : item,
            ),
        );
    };

    const move = (index: number, delta: -1 | 1) => {
        const target = index + delta;
        if (target < 0 || target >= params.length) return;
        const copy = [...params];
        const current = copy[index];
        copy[index] = copy[target];
        copy[target] = current;
        onChange(copy);
    };

    const removeAt = (index: number) => {
        onChange(params.filter((_, i) => i !== index));
    };

    return (
        <div className="space-y-3 rounded-md border p-3">
            <div className="flex items-center justify-between gap-2">
                <Label className="text-sm font-medium">
                    {t("paramsTitle")}
                </Label>
                <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => onChange([...params, createEmptyParam()])}
                >
                    <PlusIcon className="mr-1.5 size-4" />
                    {t("addParam")}
                </Button>
            </div>

            {params.length === 0 ? (
                <p className="text-xs text-muted-foreground">{t("noParams")}</p>
            ) : (
                <div className="space-y-2">
                    {params.map((param, idx) => {
                        const sessionBound = isSessionParamType(
                            param.param_type,
                        );
                        const numeric = isNumericParamType(param.param_type);
                        return (
                            <div
                                key={`${idx}-${param.param_type}`}
                                className="space-y-2 rounded-md border bg-muted/20 p-2.5"
                            >
                                <div className="flex flex-wrap items-center gap-2">
                                    <Badge
                                        variant="secondary"
                                        className="font-mono"
                                    >
                                        ${idx + 1}
                                    </Badge>
                                    <Input
                                        className="h-8 min-w-[140px] flex-1 font-mono text-xs"
                                        value={param.name}
                                        placeholder={t("paramNamePlaceholder")}
                                        onChange={(e) =>
                                            updateAt(idx, {
                                                name: e.target.value,
                                            })
                                        }
                                    />
                                    <Select
                                        value={param.param_type}
                                        onValueChange={(value) => {
                                            const nextType =
                                                value as SqlParamType;
                                            const nextSession =
                                                isSessionParamType(nextType);
                                            const defaultName =
                                                nextSession &&
                                                !param.name.trim()
                                                    ? nextType
                                                    : param.name;
                                            updateAt(idx, {
                                                param_type: nextType,
                                                name: defaultName,
                                                required: nextSession
                                                    ? false
                                                    : param.required,
                                            });
                                        }}
                                    >
                                        <SelectTrigger className="h-8 w-[200px] text-xs">
                                            <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectItem value="int">
                                                {t("typeInt")}
                                            </SelectItem>
                                            <SelectItem value="float">
                                                {t("typeFloat")}
                                            </SelectItem>
                                            <SelectItem value="bool">
                                                {t("typeBool")}
                                            </SelectItem>
                                            <SelectItem value="string">
                                                {t("typeString")}
                                            </SelectItem>
                                            <SelectItem value="datetime">
                                                {t("typeDatetime")}
                                            </SelectItem>
                                            <SelectItem value="current_user_id">
                                                {t("typeCurrentUserId")}
                                            </SelectItem>
                                            <SelectItem value="current_account">
                                                {t("typeCurrentAccount")}
                                            </SelectItem>
                                        </SelectContent>
                                    </Select>

                                    <div className="ml-auto flex items-center gap-1">
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="icon"
                                            className="size-7"
                                            disabled={idx === 0}
                                            title={t("moveUp")}
                                            onClick={() => move(idx, -1)}
                                        >
                                            <ArrowUpIcon className="size-3.5" />
                                        </Button>
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="icon"
                                            className="size-7"
                                            disabled={idx === params.length - 1}
                                            title={t("moveDown")}
                                            onClick={() => move(idx, 1)}
                                        >
                                            <ArrowDownIcon className="size-3.5" />
                                        </Button>
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="icon"
                                            className="size-7 text-destructive hover:text-destructive"
                                            title={t("removeParam")}
                                            onClick={() => removeAt(idx)}
                                        >
                                            <TrashIcon className="size-3.5" />
                                        </Button>
                                    </div>
                                </div>

                                {sessionBound ? (
                                    <p className="text-xs text-muted-foreground">
                                        {t("sessionParamAuto")}
                                    </p>
                                ) : (
                                    <div className="grid gap-2 sm:grid-cols-4">
                                        <div className="flex items-center gap-2">
                                            <Switch
                                                checked={param.required}
                                                onCheckedChange={(checked) =>
                                                    updateAt(idx, {
                                                        required: checked,
                                                    })
                                                }
                                            />
                                            <span className="text-xs">
                                                {t("paramRequired")}
                                            </span>
                                        </div>
                                        <Input
                                            className="h-8 text-xs"
                                            value={param.default_value}
                                            placeholder={t(
                                                "paramDefaultPlaceholder",
                                            )}
                                            onChange={(e) =>
                                                updateAt(idx, {
                                                    default_value:
                                                        e.target.value,
                                                })
                                            }
                                        />
                                        {numeric && (
                                            <>
                                                <Input
                                                    type="number"
                                                    className="h-8 text-xs"
                                                    value={param.min}
                                                    placeholder={t("paramMin")}
                                                    onChange={(e) =>
                                                        updateAt(idx, {
                                                            min: e.target.value,
                                                        })
                                                    }
                                                />
                                                <Input
                                                    type="number"
                                                    className="h-8 text-xs"
                                                    value={param.max}
                                                    placeholder={t("paramMax")}
                                                    onChange={(e) =>
                                                        updateAt(idx, {
                                                            max: e.target.value,
                                                        })
                                                    }
                                                />
                                            </>
                                        )}
                                    </div>
                                )}
                            </div>
                        );
                    })}
                </div>
            )}
        </div>
    );
}

interface SqlQueryEditorViewProps {
    editingItem: SqlQueryAdminItem | null;
    groups: SqlQueryGroupItem[];
    sqlI18n: (key: string) => string;
    componentI18n: (key: string) => string;
    onCancel: () => void;
    onSaved: () => Promise<void>;
}

function SqlQueryEditorView({
    editingItem,
    groups,
    sqlI18n,
    componentI18n,
    onCancel,
    onSaved,
}: SqlQueryEditorViewProps) {
    const [draft, setDraft] = useState<QueryDraft>(() =>
        editingItem ? itemToDraft(editingItem) : createEmptyDraft(),
    );
    const [saving, setSaving] = useState(false);
    const [testArgs, setTestArgs] = useState<Record<string, string>>(() =>
        buildInitialTestArgs(editingItem),
    );
    const [testing, setTesting] = useState(false);
    const [testResult, setTestResult] = useState<TestSqlQueryResp | null>(null);

    const selectableGroups = Array.from(
        new Set([
            ...groups.map((g) => g.name),
            ...(editingItem ? [editingItem.group] : []),
        ]),
    ).sort();

    const validateDraftForRequest = (requireIdentity: boolean): boolean => {
        if (requireIdentity && !draft.group.trim()) {
            toast.error(sqlI18n("groupRequired"));
            return false;
        }
        if (requireIdentity && !draft.name.trim()) {
            toast.error(sqlI18n("nameRequired"));
            return false;
        }
        if (!draft.sql.trim()) {
            toast.error(sqlI18n("sqlRequired"));
            return false;
        }
        if (draft.access === "permission" && !draft.permissionCode.trim()) {
            toast.error(sqlI18n("permissionRequired"));
            return false;
        }
        for (let i = 0; i < draft.params.length; i++) {
            const p = draft.params[i];
            if (!p.name.trim()) {
                toast.error(
                    sqlI18n("paramNameRequired").replace(
                        "{index}",
                        String(i + 1),
                    ),
                );
                return false;
            }
            if (draft.access === "public" && isSessionParamType(p.param_type)) {
                toast.error(sqlI18n("publicSessionParamForbidden"));
                return false;
            }
        }
        return true;
    };

    const handleRunTest = async () => {
        if (!validateDraftForRequest(false)) return;
        setTesting(true);
        try {
            const spec = buildSpecFromDraft(draft);
            const args: Record<string, string> = {};
            for (const p of spec.params) {
                if (!isSessionParamType(p.param_type)) {
                    const val = testArgs[p.name];
                    if (val !== undefined && val !== "") {
                        args[p.name] = val;
                    }
                }
            }
            const res = await request.post<TestSqlQueryResp>(SQL_QUERIES_TEST, {
                spec,
                args,
            });
            setTestResult(res.data);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setTesting(false);
        }
    };

    const handleSave = async () => {
        const isEdit = Boolean(editingItem);
        if (!validateDraftForRequest(!isEdit)) return;
        setSaving(true);
        try {
            const spec = buildSpecFromDraft(draft);
            const status = draft.enabled ? 1 : 0;
            if (editingItem) {
                const payload: Record<string, unknown> = {
                    description: draft.description,
                    status,
                    spec,
                };
                if (draft.effectiveStartTime.trim()) {
                    payload.effective_start_time =
                        draft.effectiveStartTime.trim();
                }
                if (draft.effectiveEndTime.trim()) {
                    payload.effective_end_time = draft.effectiveEndTime.trim();
                }
                await request.patch(
                    `${SQL_QUERIES_URL}/${editingItem.id}`,
                    payload,
                );
                toast.success(sqlI18n("updateSuccess"));
            } else {
                const payload: Record<string, unknown> = {
                    group: draft.group.trim(),
                    name: draft.name.trim(),
                    description: draft.description,
                    status,
                    spec,
                };
                if (draft.effectiveStartTime.trim()) {
                    payload.effective_start_time =
                        draft.effectiveStartTime.trim();
                }
                if (draft.effectiveEndTime.trim()) {
                    payload.effective_end_time = draft.effectiveEndTime.trim();
                }
                await request.post(SQL_QUERIES_URL, payload);
                toast.success(sqlI18n("createSuccess"));
            }
            await onSaved();
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setSaving(false);
        }
    };

    const urlParams = draft.params.filter(
        (p) => !isSessionParamType(p.param_type) && p.name.trim(),
    );
    const sessionParams = draft.params.filter((p) =>
        isSessionParamType(p.param_type),
    );
    const previewEndpoint = endpointPath(
        draft.group.trim() || "{group}",
        draft.name.trim() || "{name}",
    );

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
                        {sqlI18n("backToList")}
                    </Button>
                    <h1 className="text-xl font-semibold">
                        {editingItem
                            ? sqlI18n("editTitle").replace(
                                  "{name}",
                                  `${editingItem.group}/${editingItem.name}`,
                              )
                            : sqlI18n("createTitle")}
                    </h1>
                    <Badge variant="secondary" className="font-mono text-xs">
                        GET {previewEndpoint}
                    </Badge>
                </div>
                <div className="flex items-center gap-2">
                    <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={onCancel}
                    >
                        {sqlI18n("cancel")}
                    </Button>
                    <Button
                        type="button"
                        size="sm"
                        disabled={saving}
                        onClick={() => void handleSave()}
                    >
                        {saving ? sqlI18n("saving") : sqlI18n("save")}
                    </Button>
                </div>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle className="text-base">
                        {sqlI18n("basicSection")}
                    </CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="grid gap-3 md:grid-cols-3">
                        <div className="space-y-1.5">
                            <Label>{sqlI18n("colGroup")}</Label>
                            <Select
                                disabled={
                                    Boolean(editingItem) ||
                                    selectableGroups.length === 0
                                }
                                value={draft.group}
                                onValueChange={(value) =>
                                    setDraft((d) => ({
                                        ...d,
                                        group: value,
                                    }))
                                }
                            >
                                <SelectTrigger className="w-full font-mono">
                                    <SelectValue
                                        placeholder={sqlI18n(
                                            "groupSelectPlaceholder",
                                        )}
                                    />
                                </SelectTrigger>
                                <SelectContent>
                                    {selectableGroups.map((g) => (
                                        <SelectItem
                                            key={g}
                                            value={g}
                                            className="font-mono"
                                        >
                                            {g}
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                        <div className="space-y-1.5">
                            <Label>{sqlI18n("colName")}</Label>
                            <Input
                                className="font-mono"
                                disabled={Boolean(editingItem)}
                                value={draft.name}
                                placeholder={sqlI18n("namePlaceholder")}
                                onChange={(e) =>
                                    setDraft((d) => ({
                                        ...d,
                                        name: e.target.value,
                                    }))
                                }
                            />
                        </div>
                        <div className="space-y-1.5">
                            <Label>{sqlI18n("description")}</Label>
                            <Input
                                value={draft.description}
                                placeholder={sqlI18n("descriptionPlaceholder")}
                                onChange={(e) =>
                                    setDraft((d) => ({
                                        ...d,
                                        description: e.target.value,
                                    }))
                                }
                            />
                        </div>
                    </div>

                    {!editingItem && selectableGroups.length === 0 && (
                        <div className="rounded-md border border-dashed p-3 text-sm text-muted-foreground">
                            {sqlI18n("noGroupsHint")}
                        </div>
                    )}

                    <div className="grid items-end gap-3 md:grid-cols-3">
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
                            <Label>{sqlI18n("enabled")}</Label>
                        </div>
                        <div className="space-y-1.5">
                            <Label>{sqlI18n("effectiveStartTime")}</Label>
                            <DateTimePicker
                                clearable
                                i18n={componentI18n}
                                date={draft.effectiveStartTime || null}
                                placeholder={sqlI18n("timePlaceholder")}
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
                            <Label>{sqlI18n("effectiveEndTime")}</Label>
                            <DateTimePicker
                                clearable
                                i18n={componentI18n}
                                date={draft.effectiveEndTime || null}
                                placeholder={sqlI18n("timePlaceholder")}
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

                    <div className="grid gap-3 md:grid-cols-3">
                        <div className="space-y-1.5">
                            <Label>{sqlI18n("access")}</Label>
                            <Select
                                value={draft.access}
                                onValueChange={(value) =>
                                    setDraft((d) => ({
                                        ...d,
                                        access: value as SqlAccessLevel,
                                    }))
                                }
                            >
                                <SelectTrigger className="w-full">
                                    <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="admin">
                                        {sqlI18n("accessAdmin")}
                                    </SelectItem>
                                    <SelectItem value="permission">
                                        {sqlI18n("accessPermission")}
                                    </SelectItem>
                                    <SelectItem value="login">
                                        {sqlI18n("accessLogin")}
                                    </SelectItem>
                                    <SelectItem value="public">
                                        {sqlI18n("accessPublic")}
                                    </SelectItem>
                                </SelectContent>
                            </Select>
                        </div>

                        {draft.access === "permission" && (
                            <div className="space-y-1.5">
                                <Label>{sqlI18n("permissionCode")}</Label>
                                <Input
                                    className="font-mono"
                                    value={draft.permissionCode}
                                    placeholder={sqlI18n(
                                        "permissionCodePlaceholder",
                                    )}
                                    onChange={(e) =>
                                        setDraft((d) => ({
                                            ...d,
                                            permissionCode: e.target.value,
                                        }))
                                    }
                                />
                            </div>
                        )}

                        <div className="space-y-1.5">
                            <Label>{sqlI18n("resultMode")}</Label>
                            <Select
                                value={draft.resultMode}
                                onValueChange={(value) =>
                                    setDraft((d) => ({
                                        ...d,
                                        resultMode: value as SqlResultMode,
                                    }))
                                }
                            >
                                <SelectTrigger className="w-full">
                                    <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value="list">
                                        {sqlI18n("modeList")}
                                    </SelectItem>
                                    <SelectItem value="single">
                                        {sqlI18n("modeSingle")}
                                    </SelectItem>
                                </SelectContent>
                            </Select>
                        </div>
                    </div>

                    <div className="grid gap-3 md:grid-cols-3">
                        <div className="space-y-1.5">
                            <Label>{sqlI18n("timeoutMs")}</Label>
                            <Input
                                type="number"
                                min={100}
                                max={15000}
                                value={draft.timeoutMs}
                                onChange={(e) =>
                                    setDraft((d) => ({
                                        ...d,
                                        timeoutMs: e.target.value,
                                    }))
                                }
                            />
                        </div>
                        <div className="space-y-1.5">
                            <Label>{sqlI18n("maxRows")}</Label>
                            <Input
                                type="number"
                                min={1}
                                max={1000}
                                value={draft.maxRows}
                                onChange={(e) =>
                                    setDraft((d) => ({
                                        ...d,
                                        maxRows: e.target.value,
                                    }))
                                }
                            />
                        </div>
                        <div className="space-y-1.5">
                            <Label>{sqlI18n("cacheTtlSecs")}</Label>
                            <Input
                                type="number"
                                min={0}
                                max={86400}
                                value={draft.cacheTtlSecs}
                                onChange={(e) =>
                                    setDraft((d) => ({
                                        ...d,
                                        cacheTtlSecs: e.target.value,
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
                        {sqlI18n("sqlSection")}
                    </CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="space-y-1.5">
                        <Label>{sqlI18n("sqlLabel")}</Label>
                        <Textarea
                            rows={8}
                            className="font-mono text-xs"
                            value={draft.sql}
                            placeholder={sqlI18n("sqlPlaceholder")}
                            onChange={(e) =>
                                setDraft((d) => ({
                                    ...d,
                                    sql: e.target.value,
                                }))
                            }
                        />
                        <p className="text-xs text-muted-foreground">
                            {sqlI18n("sqlHint")}
                        </p>
                    </div>

                    <ParamBuilder
                        params={draft.params}
                        t={sqlI18n}
                        onChange={(nextParams) =>
                            setDraft((d) => ({ ...d, params: nextParams }))
                        }
                    />
                </CardContent>
            </Card>

            <Card>
                <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
                    <CardTitle className="text-base">
                        {sqlI18n("testTitle")}
                    </CardTitle>
                    <Button
                        type="button"
                        variant="secondary"
                        size="sm"
                        disabled={testing}
                        onClick={() => void handleRunTest()}
                    >
                        <PlayIcon className="mr-1.5 size-3.5" />
                        {testing ? sqlI18n("testing") : sqlI18n("runTest")}
                    </Button>
                </CardHeader>
                <CardContent className="space-y-3">
                    {urlParams.length > 0 ? (
                        <div className="grid gap-2 md:grid-cols-2 lg:grid-cols-3">
                            {urlParams.map((p, i) => {
                                const paramName = p.name.trim();
                                return (
                                    <div
                                        key={`${paramName}-${i}`}
                                        className="flex items-center gap-2"
                                    >
                                        <Badge
                                            variant="outline"
                                            className="shrink-0 font-mono text-xs"
                                        >
                                            {paramName}
                                        </Badge>
                                        <Input
                                            className="h-8 text-xs"
                                            value={testArgs[paramName] ?? ""}
                                            placeholder={
                                                p.default_value ||
                                                sqlI18n(
                                                    "testArgPlaceholder",
                                                ).replace("{name}", paramName)
                                            }
                                            onChange={(e) =>
                                                setTestArgs((prev) => ({
                                                    ...prev,
                                                    [paramName]: e.target.value,
                                                }))
                                            }
                                        />
                                    </div>
                                );
                            })}
                        </div>
                    ) : (
                        <p className="text-xs text-muted-foreground">
                            {sqlI18n("testNoArgs")}
                            {sessionParams.length > 0 &&
                                ` ${sqlI18n("testSessionNote")}`}
                        </p>
                    )}

                    {testResult ? (
                        <div className="space-y-1.5">
                            <div className="text-xs text-muted-foreground">
                                {sqlI18n("testStats")
                                    .replace(
                                        "{elapsed}",
                                        String(testResult.elapsed_ms),
                                    )
                                    .replace(
                                        "{rows}",
                                        String(testResult.row_count),
                                    )}
                            </div>
                            <pre className="max-h-96 overflow-auto rounded-md bg-muted p-3 text-xs">
                                {JSON.stringify(testResult.result, null, 2)}
                            </pre>
                        </div>
                    ) : (
                        <p className="text-xs text-muted-foreground">
                            {sqlI18n("testEmptyHint")}
                        </p>
                    )}
                </CardContent>
            </Card>

            <div className="flex justify-end gap-2">
                <Button type="button" variant="outline" onClick={onCancel}>
                    {sqlI18n("cancel")}
                </Button>
                <Button
                    type="button"
                    disabled={saving}
                    onClick={() => void handleSave()}
                >
                    {saving ? sqlI18n("saving") : sqlI18n("save")}
                </Button>
            </div>
        </div>
    );
}

interface SqlQueryGroupCardProps {
    groups: SqlQueryGroupItem[];
    loading: boolean;
    sqlI18n: (key: string) => string;
    onRefresh: () => Promise<void>;
}

function SqlQueryGroupCard({
    groups,
    loading,
    sqlI18n,
    onRefresh,
}: SqlQueryGroupCardProps) {
    const [groupName, setGroupName] = useState("");
    const [groupDesc, setGroupDesc] = useState("");
    const [creating, setCreating] = useState(false);
    const [deleteGroup, setDeleteGroup] = useState<SqlQueryGroupItem | null>(
        null,
    );

    const handleCreateGroup = async () => {
        const trimmed = groupName.trim();
        if (!trimmed) {
            toast.error(sqlI18n("groupRequired"));
            return;
        }
        setCreating(true);
        try {
            await request.post(SQL_QUERIES_GROUPS, {
                name: trimmed,
                description: groupDesc.trim(),
            });
            setGroupName("");
            setGroupDesc("");
            toast.success(sqlI18n("groupCreated"));
            await onRefresh();
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setCreating(false);
        }
    };

    const handleDeleteGroupConfirm = async () => {
        if (!deleteGroup) return;
        const target = deleteGroup;
        setDeleteGroup(null);
        try {
            await request.delete(`${SQL_QUERIES_GROUPS}/${target.id}`);
            toast.success(sqlI18n("groupDeleted"));
            await onRefresh();
        } catch (err) {
            toast.error(formatError(err));
        }
    };

    return (
        <>
            <Card>
                <CardHeader>
                    <CardTitle className="text-base">
                        {sqlI18n("groupSection")} ({groups.length})
                    </CardTitle>
                    <p className="text-xs text-muted-foreground">
                        {sqlI18n("groupSectionHint")}
                    </p>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="flex flex-wrap items-center gap-2">
                        <Input
                            className="w-full font-mono sm:w-64"
                            value={groupName}
                            placeholder={sqlI18n("groupNamePlaceholder")}
                            onChange={(e) => setGroupName(e.target.value)}
                        />
                        <Input
                            className="min-w-[200px] flex-1"
                            value={groupDesc}
                            placeholder={sqlI18n("groupDescPlaceholder")}
                            onChange={(e) => setGroupDesc(e.target.value)}
                        />
                        <Button
                            type="button"
                            size="sm"
                            disabled={creating}
                            onClick={() => void handleCreateGroup()}
                        >
                            <PlusIcon className="mr-1.5 size-4" />
                            {creating
                                ? sqlI18n("creatingGroup")
                                : sqlI18n("createGroup")}
                        </Button>
                    </div>

                    {loading && groups.length === 0 ? (
                        <Loading />
                    ) : groups.length === 0 ? (
                        <div className="py-4 text-center text-sm text-muted-foreground">
                            {sqlI18n("groupEmpty")}
                        </div>
                    ) : (
                        <div className="overflow-x-auto">
                            <Table>
                                <TableHeader>
                                    <TableRow>
                                        <TableHead>
                                            {sqlI18n("colGroup")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("description")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("colQueryCount")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("colModified")}
                                        </TableHead>
                                        <TableHead className="text-right">
                                            {sqlI18n("colAction")}
                                        </TableHead>
                                    </TableRow>
                                </TableHeader>
                                <TableBody>
                                    {groups.map((g) => (
                                        <TableRow key={g.id}>
                                            <TableCell className="align-middle">
                                                <Badge
                                                    variant="secondary"
                                                    className="font-mono text-xs"
                                                >
                                                    {g.name}
                                                </Badge>
                                            </TableCell>
                                            <TableCell className="align-middle text-xs text-muted-foreground">
                                                {g.description || "-"}
                                            </TableCell>
                                            <TableCell className="align-middle text-xs">
                                                {g.query_count}
                                            </TableCell>
                                            <TableCell className="align-middle text-xs text-muted-foreground">
                                                {formatDate(g.modified)}
                                            </TableCell>
                                            <TableCell className="align-middle text-right">
                                                <Button
                                                    type="button"
                                                    variant="ghost"
                                                    size="icon"
                                                    className="size-8 text-destructive hover:text-destructive"
                                                    disabled={g.query_count > 0}
                                                    title={
                                                        g.query_count > 0
                                                            ? sqlI18n(
                                                                  "groupInUseHint",
                                                              )
                                                            : sqlI18n(
                                                                  "deleteGroupTitle",
                                                              )
                                                    }
                                                    onClick={() =>
                                                        setDeleteGroup(g)
                                                    }
                                                >
                                                    <TrashIcon className="size-4" />
                                                </Button>
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
                open={Boolean(deleteGroup)}
                onOpenChange={(open) => {
                    if (!open) setDeleteGroup(null);
                }}
            >
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>
                            {sqlI18n("deleteGroupTitle")}
                        </AlertDialogTitle>
                        <AlertDialogDescription>
                            {sqlI18n("deleteGroupConfirm").replace(
                                "{name}",
                                deleteGroup?.name ?? "",
                            )}
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>
                            {sqlI18n("cancel")}
                        </AlertDialogCancel>
                        <AlertDialogAction
                            onClick={() => void handleDeleteGroupConfirm()}
                        >
                            {sqlI18n("deleteGroupTitle")}
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </>
    );
}

export default function SqlQueries() {
    const commonI18n = useI18n("common");
    const sqlI18n = useI18n("sqlQueries");
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
    const isSu = isSuperAdminRole(user.roles);

    const [items, setItems] = useState<SqlQueryAdminItem[]>([]);
    const [groups, setGroups] = useState<SqlQueryGroupItem[]>([]);
    const [loaded, setLoaded] = useState(false);
    const [refreshing, setRefreshing] = useState(false);
    const [featureDisabled, setFeatureDisabled] = useState(false);
    const [deleteTarget, setDeleteTarget] = useState<SqlQueryAdminItem | null>(
        null,
    );
    const [groupFilter, setGroupFilter] = useState<string>("__all__");

    const filterGroupNames = Array.from(
        new Set([
            ...groups.map((g) => g.name),
            ...items.map((item) => item.group),
        ]),
    ).sort();
    const activeGroupFilter =
        groupFilter === "__all__" || filterGroupNames.includes(groupFilter)
            ? groupFilter
            : "__all__";
    const visibleItems =
        activeGroupFilter === "__all__"
            ? items
            : items.filter((item) => item.group === activeGroupFilter);

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
            const [queriesRes, groupsRes] = await Promise.all([
                request.get<SqlQueryAdminItem[]>(SQL_QUERIES_URL),
                request.get<SqlQueryGroupItem[]>(SQL_QUERIES_GROUPS),
            ]);
            setItems(queriesRes.data);
            setGroups(groupsRes.data);
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
                    title: sqlI18n("title"),
                    icon: <DatabaseIcon className="size-4" />,
                    url: SQL_QUERIES,
                },
                {
                    title: sqlI18n("createTitle"),
                },
            ]);
        } else if (mode === "edit") {
            setPageHeaderNavigation([
                {
                    title: sqlI18n("title"),
                    icon: <DatabaseIcon className="size-4" />,
                    url: SQL_QUERIES,
                },
                {
                    title: editingTarget
                        ? sqlI18n("editTitle").replace(
                              "{name}",
                              `${editingTarget.group}/${editingTarget.name}`,
                          )
                        : sqlI18n("edit"),
                },
            ]);
        } else {
            setPageHeaderNavigation([
                {
                    title: sqlI18n("title"),
                    icon: <DatabaseIcon className="size-4" />,
                },
            ]);
        }
        return () => {
            resetPageHeaderNavigation();
        };
    }, [
        mode,
        editingTarget,
        sqlI18n,
        setPageHeaderNavigation,
        resetPageHeaderNavigation,
    ]);

    useEffect(() => {
        if (!initialized || !canAccess) return;
        let cancelled = false;
        Promise.all([
            request.get<SqlQueryAdminItem[]>(SQL_QUERIES_URL),
            request.get<SqlQueryGroupItem[]>(SQL_QUERIES_GROUPS),
        ])
            .then(([queriesRes, groupsRes]) => {
                if (!cancelled) {
                    setItems(queriesRes.data);
                    setGroups(groupsRes.data);
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

    const handleSaved = async () => {
        await refreshData();
        setSearchParams({});
    };

    const handleDeleteConfirm = async () => {
        if (!deleteTarget) return;
        const target = deleteTarget;
        setDeleteTarget(null);
        try {
            await request.delete(`${SQL_QUERIES_URL}/${target.id}`);
            toast.success(sqlI18n("deleteSuccess"));
            await refreshData();
        } catch (err) {
            toast.error(formatError(err));
        }
    };

    const handleCopyEndpoint = async (group: string, name: string) => {
        const path = endpointPath(group, name);
        try {
            await navigator.clipboard.writeText(path);
            toast.success(sqlI18n("copied").replace("{path}", path));
        } catch {
            toast.error(sqlI18n("copyFailed"));
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
                {sqlI18n("featureDisabled")}
            </div>
        );
    }

    if (mode === "create" || mode === "edit") {
        if (!isSu) {
            return (
                <div className="w-full space-y-6 p-2 md:p-4">
                    <Card>
                        <CardContent className="flex flex-col items-center gap-4 py-12 text-center">
                            <p className="text-sm text-muted-foreground">
                                {commonI18n("noPermission")}
                            </p>
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                onClick={backToList}
                            >
                                <ArrowLeftIcon className="mr-1.5 size-4" />
                                {sqlI18n("backToList")}
                            </Button>
                        </CardContent>
                    </Card>
                </div>
            );
        }
    }

    if (mode === "create") {
        return (
            <SqlQueryEditorView
                key="create"
                editingItem={null}
                groups={groups}
                sqlI18n={sqlI18n}
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
                                {sqlI18n("notFound")}
                            </p>
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                onClick={backToList}
                            >
                                <ArrowLeftIcon className="mr-1.5 size-4" />
                                {sqlI18n("backToList")}
                            </Button>
                        </CardContent>
                    </Card>
                </div>
            );
        }
        return (
            <SqlQueryEditorView
                key={`edit-${editingTarget.id}`}
                editingItem={editingTarget}
                groups={groups}
                sqlI18n={sqlI18n}
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
                        {sqlI18n("title")}
                    </h1>
                    <p className="mt-1 text-sm text-muted-foreground">
                        {sqlI18n("descriptionHint")}
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
                        {sqlI18n("refresh")}
                    </Button>
                    {isSu && (
                        <Button
                            type="button"
                            size="sm"
                            onClick={() => setSearchParams({ mode: "create" })}
                        >
                            <PlusIcon className="mr-1.5 size-4" />
                            {sqlI18n("create")}
                        </Button>
                    )}
                </div>
            </div>

            <SqlQueryGroupCard
                groups={groups}
                loading={loading}
                sqlI18n={sqlI18n}
                onRefresh={refreshData}
            />

            <Card>
                <CardHeader className="flex flex-row flex-wrap items-center justify-between gap-2">
                    <CardTitle className="text-base">
                        {sqlI18n("title")} ({visibleItems.length})
                    </CardTitle>
                    {filterGroupNames.length > 0 && (
                        <Select
                            value={activeGroupFilter}
                            onValueChange={setGroupFilter}
                        >
                            <SelectTrigger className="h-8 w-44 text-xs">
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="__all__">
                                    {sqlI18n("allGroups")}
                                </SelectItem>
                                {filterGroupNames.map((g) => (
                                    <SelectItem key={g} value={g}>
                                        {g}
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                    )}
                </CardHeader>
                <CardContent>
                    {loading && items.length === 0 ? (
                        <Loading />
                    ) : visibleItems.length === 0 ? (
                        <div className="py-8 text-center text-sm text-muted-foreground">
                            {sqlI18n("empty")}
                        </div>
                    ) : (
                        <div className="overflow-x-auto">
                            <Table>
                                <TableHeader>
                                    <TableRow>
                                        <TableHead>
                                            {sqlI18n("colGroup")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("colName")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("colEndpoint")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("colAccess")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("colResultMode")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("colCache")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("colStatus")}
                                        </TableHead>
                                        <TableHead>
                                            {sqlI18n("colModified")}
                                        </TableHead>
                                        {isSu && (
                                            <TableHead className="text-right">
                                                {sqlI18n("colAction")}
                                            </TableHead>
                                        )}
                                    </TableRow>
                                </TableHeader>
                                <TableBody>
                                    {visibleItems.map((item) => {
                                        const ep = endpointPath(
                                            item.group,
                                            item.name,
                                        );
                                        const spec = item.spec;
                                        return (
                                            <TableRow key={item.id}>
                                                <TableCell className="align-top">
                                                    <Badge
                                                        variant="secondary"
                                                        className="font-mono text-xs"
                                                    >
                                                        {item.group}
                                                    </Badge>
                                                </TableCell>
                                                <TableCell className="align-top">
                                                    <div className="font-mono text-sm font-medium">
                                                        {item.name}
                                                    </div>
                                                    {item.description && (
                                                        <div className="max-w-xs truncate text-xs text-muted-foreground">
                                                            {item.description}
                                                        </div>
                                                    )}
                                                </TableCell>
                                                <TableCell className="align-top">
                                                    <div className="flex items-center gap-1">
                                                        <code className="rounded bg-muted px-1.5 py-0.5 text-xs">
                                                            {ep}
                                                        </code>
                                                        <Button
                                                            type="button"
                                                            variant="ghost"
                                                            size="icon"
                                                            className="size-6"
                                                            title={sqlI18n(
                                                                "copyEndpoint",
                                                            )}
                                                            onClick={() =>
                                                                void handleCopyEndpoint(
                                                                    item.group,
                                                                    item.name,
                                                                )
                                                            }
                                                        >
                                                            <CopyIcon className="size-3.5" />
                                                        </Button>
                                                    </div>
                                                </TableCell>
                                                <TableCell className="align-top">
                                                    {spec ? (
                                                        <Badge variant="outline">
                                                            {formatAccessLabel(
                                                                spec.access,
                                                                spec.permission_code,
                                                                sqlI18n,
                                                            )}
                                                        </Badge>
                                                    ) : (
                                                        <Badge variant="destructive">
                                                            {sqlI18n(
                                                                "invalidSpec",
                                                            )}
                                                        </Badge>
                                                    )}
                                                </TableCell>
                                                <TableCell className="align-top text-xs">
                                                    {spec
                                                        ? spec.result_mode
                                                        : "-"}
                                                </TableCell>
                                                <TableCell className="align-top text-xs">
                                                    {spec
                                                        ? spec.cache_ttl_secs >
                                                          0
                                                            ? sqlI18n(
                                                                  "cacheSeconds",
                                                              ).replace(
                                                                  "{ttl}",
                                                                  String(
                                                                      spec.cache_ttl_secs,
                                                                  ),
                                                              )
                                                            : sqlI18n(
                                                                  "cacheOff",
                                                              )
                                                        : "-"}
                                                </TableCell>
                                                <TableCell className="align-top">
                                                    <StatusBadge
                                                        status={String(
                                                            item.status,
                                                        )}
                                                        i18nModel={modelI18n}
                                                    />
                                                </TableCell>
                                                <TableCell className="align-top text-xs text-muted-foreground">
                                                    {formatDate(item.modified)}
                                                </TableCell>
                                                {isSu && (
                                                    <TableCell className="align-top text-right">
                                                        <div className="flex justify-end gap-1">
                                                            <Button
                                                                type="button"
                                                                variant="outline"
                                                                size="sm"
                                                                onClick={() =>
                                                                    setSearchParams(
                                                                        {
                                                                            mode: "edit",
                                                                            id: String(
                                                                                item.id,
                                                                            ),
                                                                        },
                                                                    )
                                                                }
                                                            >
                                                                <PencilIcon className="mr-1 size-3.5" />
                                                                {sqlI18n(
                                                                    "edit",
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
                                                )}
                                            </TableRow>
                                        );
                                    })}
                                </TableBody>
                            </Table>
                        </div>
                    )}
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
                            {sqlI18n("deleteTitle")}
                        </AlertDialogTitle>
                        <AlertDialogDescription>
                            {sqlI18n("deleteConfirm")
                                .replace("{group}", deleteTarget?.group ?? "")
                                .replace("{name}", deleteTarget?.name ?? "")}
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel>
                            {sqlI18n("cancel")}
                        </AlertDialogCancel>
                        <AlertDialogAction
                            onClick={() => void handleDeleteConfirm()}
                        >
                            {sqlI18n("deleteTitle")}
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </div>
    );
}
