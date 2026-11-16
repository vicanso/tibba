import { create } from "zustand";
import request from "@/helpers/request";
import sha256 from "crypto-js/sha256";
import {
    UsersIcon,
    LucideIcon,
    FileIcon,
    HomeIcon,
    CogIcon,
    UploadIcon,
    ActivityIcon,
    ShieldIcon,
    ScrollTextIcon,
    DatabaseIcon,
    KeyRoundIcon,
} from "lucide-react";
import {
    APPS,
    AUDIT_LOGS,
    FILE_UPLOADER,
    HOME,
    JOBS,
    MODEL,
    RBAC,
    SQL_QUERIES,
} from "@/constants/route";
import { hasPermission, isAdminRole } from "@/helpers/util";

import {
    USER_LOGIN,
    USER_LOGIN_MFA,
    USER_LOGIN_TOKEN,
    USER_LOGOUT,
    USER_ME,
    USER_REGISTER,
    USER_PROFILE,
    USER_PASSWORD,
    USER_AUDIT_LOGS,
    USER_REFRESH,
    USER_EMAIL_VERIFY_REQUEST,
    USER_EMAIL_VERIFY_CONFIRM,
    USER_PASSWORD_RESET_REQUEST,
    USER_PASSWORD_RESET_CONFIRM,
    USER_TOTP_STATUS,
    USER_TOTP_ENROLL,
    USER_TOTP_ACTIVATE,
    USER_TOTP_DISABLE,
    USER_API_KEYS,
    USER_OAUTH_LINKS,
    USER_OAUTH_UNLINK,
} from "@/constants/url";

export interface UserSession {
    account: string;
    expired_at: string;
    issued_at: string;
    time: string;
    can_renew: boolean;
    has_password: boolean;
    nickname?: string;
    phone?: string;
    email?: string;
    avatar?: string;
    roles: string[];
    groups: string[];
    permissions: string[];
}

export interface OAuthLinkItem {
    provider: string;
    provider_login: string;
    provider_email: string;
    created: string;
}

export interface OAuthLinksResp {
    providers: string[];
    links: OAuthLinkItem[];
}

export interface AuditLogItem {
    id: number;
    user_id?: number | null;
    action: string;
    target_type: string;
    target_id: string;
    detail: Record<string, unknown>;
    request_id: string;
    ip: string;
    user_agent: string;
    created: string;
}

export type LoginResult =
    | { mfaRequired: true; mfaToken: string }
    | { mfaRequired: false; user: UserSession };

export interface TotpStatus {
    enrolled: boolean;
    enabled: boolean;
}

export interface TotpEnrollResp {
    secret: string;
    otpauth_uri: string;
}

export interface ApiKeyItem {
    id: number;
    name: string;
    key_prefix: string;
    last_used_at?: string | null;
    expires_at?: string | null;
    created: string;
}

export interface CreateApiKeyResp {
    id: number;
    name: string;
    key_prefix: string;
    token: string;
    expires_in_days?: number | null;
}

// 导航菜单项接口
export interface NavItem {
    title: string;
    url: string;
    disabled?: boolean;
    icon: LucideIcon;
}

// 导航组接口
export interface NavGroup {
    title: string;
    url: string;
    items: NavItem[];
}

function getMainNav(roles: string[], permissions: string[]): NavGroup[] {
    const admin = isAdminRole(roles);
    const groups: NavGroup[] = [
        {
            title: "workspace",
            url: "#",
            items: [
                {
                    title: "home",
                    url: HOME,
                    icon: HomeIcon,
                },
                {
                    title: "fileUploader",
                    url: FILE_UPLOADER,
                    icon: UploadIcon,
                },
            ],
        },
    ];

    const modelItems: NavItem[] = [];
    if (
        admin ||
        hasPermission(permissions, "model:user:read") ||
        hasPermission(permissions, "model:user:write")
    ) {
        modelItems.push({
            title: "user",
            url: `${MODEL}/user`,
            icon: UsersIcon,
        });
    }
    if (
        admin ||
        hasPermission(permissions, "model:configuration:read") ||
        hasPermission(permissions, "model:configuration:write")
    ) {
        modelItems.push({
            title: "config",
            url: `${MODEL}/configuration`,
            icon: CogIcon,
        });
    }
    if (admin) {
        modelItems.push({
            title: "file",
            url: `${MODEL}/file`,
            icon: FileIcon,
        });
    }
    if (modelItems.length > 0) {
        groups.push({
            title: "modelFeature",
            url: "#",
            items: modelItems,
        });
    }

    const systemItems: NavItem[] = [];
    if (
        admin ||
        hasPermission(permissions, "rbac:read") ||
        hasPermission(permissions, "rbac:write")
    ) {
        systemItems.push({
            title: "rbac",
            url: RBAC,
            icon: ShieldIcon,
        });
    }
    if (admin || hasPermission(permissions, "audit:read")) {
        systemItems.push({
            title: "auditLogs",
            url: AUDIT_LOGS,
            icon: ScrollTextIcon,
        });
    }
    if (admin) {
        systemItems.push({
            title: "jobs",
            url: JOBS,
            icon: ActivityIcon,
        });
    }
    if (admin) {
        systemItems.push({
            title: "sqlQueries",
            url: SQL_QUERIES,
            icon: DatabaseIcon,
        });
        systemItems.push({
            title: "apps",
            url: APPS,
            icon: KeyRoundIcon,
        });
    }
    if (systemItems.length > 0) {
        groups.push({
            title: "systemFeature",
            url: "#",
            items: systemItems,
        });
    }

    return groups;
}

interface RawUserMeResp {
    account?: string;
    expired_at?: string;
    issued_at?: string;
    time?: string;
    can_renew?: boolean;
    has_password?: boolean;
    nickname?: string | null;
    phone?: string | null;
    email?: string | null;
    avatar?: string | null;
    roles?: string[] | null;
    groups?: string[] | null;
    permissions?: string[] | null;
}

function normalizeUserSession(raw?: RawUserMeResp | null): UserSession {
    return {
        account: raw?.account || "",
        expired_at: raw?.expired_at || "",
        issued_at: raw?.issued_at || "",
        time: raw?.time || "",
        can_renew: Boolean(raw?.can_renew),
        has_password: Boolean(raw?.has_password),
        nickname: raw?.nickname || undefined,
        phone: raw?.phone || undefined,
        email: raw?.email || undefined,
        avatar: raw?.avatar || undefined,
        roles: raw?.roles ?? [],
        groups: raw?.groups ?? [],
        permissions: raw?.permissions ?? [],
    };
}

interface UserState {
    data: UserSession;
    initialized: boolean;
    mainNav: NavGroup[];
    updateMainNav: (
        _app: string,
        roles: string[],
        permissions?: string[],
    ) => Promise<void>;
    resetMainNav: () => void;
    fetch: () => Promise<UserSession>;
    signUp: (account: string, password: string) => Promise<void>;
    login: (account: string, password: string) => Promise<LoginResult>;
    loginMfa: (mfaToken: string, code: string) => Promise<UserSession>;
    logout: () => Promise<void>;
    refresh: () => Promise<void>;
    updateProfile: (profile: {
        nickname?: string;
        phone?: string;
        email?: string;
        avatar?: string;
    }) => Promise<void>;
    requestPasswordReset: (account: string) => Promise<void>;
    confirmPasswordReset: (token: string, rawPassword: string) => Promise<void>;
    requestEmailVerify: () => Promise<void>;
    confirmEmailVerify: (token: string) => Promise<void>;
    getTotpStatus: () => Promise<TotpStatus>;
    enrollTotp: () => Promise<TotpEnrollResp>;
    activateTotp: (code: string) => Promise<{ recovery_codes: string[] }>;
    disableTotp: (code: string) => Promise<void>;
    listApiKeys: () => Promise<ApiKeyItem[]>;
    createApiKey: (
        name: string,
        expiresInDays?: number,
    ) => Promise<CreateApiKeyResp>;
    revokeApiKey: (id: number) => Promise<void>;
    changePassword: (
        oldRawPassword: string | undefined,
        newRawPassword: string,
    ) => Promise<void>;
    listOAuthLinks: () => Promise<OAuthLinksResp>;
    unlinkOAuth: (provider: string) => Promise<void>;
    listMyAuditLogs: (limit?: number) => Promise<AuditLogItem[]>;
}

const defaultUser: UserSession = {
    account: "",
    expired_at: "",
    issued_at: "",
    time: "",
    can_renew: false,
    has_password: false,
    roles: [],
    groups: [],
    permissions: [],
};

const useUserState = create<UserState>((set, get) => ({
    data: defaultUser,
    initialized: false,
    mainNav: [],
    fetch: async () => {
        const { data } = await request.get<RawUserMeResp>(USER_ME);
        const user = normalizeUserSession(data);
        set({
            initialized: true,
            data: user,
        });
        return user;
    },
    updateMainNav: async (
        _app: string,
        roles: string[],
        permissions: string[] = [],
    ) => {
        set({
            mainNav: getMainNav(roles, permissions),
        });
    },
    resetMainNav: () => {
        set({
            mainNav: [],
        });
    },
    signUp: async (account: string, password: string) => {
        await request.post(USER_REGISTER, {
            account,
            password: sha256(password).toString(),
        });
    },
    login: async (account: string, password: string) => {
        const { data } = await request.get<{
            ts: number;
            hash: string;
            token: string;
        }>(USER_LOGIN_TOKEN);
        // 仅对明文口令做一层 sha256（定长化 / 不发送明文）；服务端再套 Argon2 校验。
        // ts/hash/token 仍随请求发送，用于服务端登录令牌的防重放校验。
        const { data: resp } = await request.post<
            RawUserMeResp | { mfa_required: true; mfa_token: string }
        >(USER_LOGIN, {
            ts: data.ts,
            hash: data.hash,
            token: data.token,
            account,
            password: sha256(password).toString(),
        });
        if ("mfa_required" in resp && resp.mfa_required) {
            return {
                mfaRequired: true,
                mfaToken: resp.mfa_token,
            };
        }
        const user = normalizeUserSession(resp as RawUserMeResp);
        set({
            initialized: true,
            data: user,
        });
        return {
            mfaRequired: false,
            user,
        };
    },
    loginMfa: async (mfaToken: string, code: string) => {
        const { data } = await request.post<RawUserMeResp>(USER_LOGIN_MFA, {
            mfa_token: mfaToken,
            code: code.trim(),
        });
        const user = normalizeUserSession(data);
        set({
            initialized: true,
            data: user,
        });
        return user;
    },
    logout: async () => {
        await request.delete(USER_LOGOUT);
        set({
            initialized: true,
            data: defaultUser,
            mainNav: [],
        });
    },
    updateProfile: async (profile: {
        nickname?: string;
        phone?: string;
        email?: string;
        avatar?: string;
    }) => {
        await request.patch(USER_PROFILE, profile);
        await get().fetch();
    },
    refresh: async () => {
        await request.patch(USER_REFRESH);
    },
    requestPasswordReset: async (account: string) => {
        await request.post(USER_PASSWORD_RESET_REQUEST, { account });
    },
    confirmPasswordReset: async (token: string, rawPassword: string) => {
        await request.post(USER_PASSWORD_RESET_CONFIRM, {
            token: token.trim(),
            password: sha256(rawPassword).toString(),
        });
    },
    requestEmailVerify: async () => {
        await request.post(USER_EMAIL_VERIFY_REQUEST);
    },
    confirmEmailVerify: async (token: string) => {
        await request.post(USER_EMAIL_VERIFY_CONFIRM, { token: token.trim() });
    },
    getTotpStatus: async () => {
        const { data } = await request.get<TotpStatus>(USER_TOTP_STATUS);
        return data;
    },
    enrollTotp: async () => {
        const { data } = await request.post<TotpEnrollResp>(USER_TOTP_ENROLL);
        return data;
    },
    activateTotp: async (code: string) => {
        const { data } = await request.post<{ recovery_codes: string[] }>(
            USER_TOTP_ACTIVATE,
            { code: code.trim() },
        );
        return data;
    },
    disableTotp: async (code: string) => {
        await request.post(USER_TOTP_DISABLE, { code: code.trim() });
    },
    listApiKeys: async () => {
        const { data } = await request.get<ApiKeyItem[]>(USER_API_KEYS);
        return data;
    },
    createApiKey: async (name: string, expiresInDays?: number) => {
        const body: { name: string; expires_in_days?: number } = { name };
        if (expiresInDays && expiresInDays > 0) {
            body.expires_in_days = expiresInDays;
        }
        const { data } = await request.post<CreateApiKeyResp>(
            USER_API_KEYS,
            body,
        );
        return data;
    },
    revokeApiKey: async (id: number) => {
        await request.delete(`${USER_API_KEYS}/${id}`);
    },
    changePassword: async (
        oldRawPassword: string | undefined,
        newRawPassword: string,
    ) => {
        const payload: { old_password?: string; new_password: string } = {
            new_password: sha256(newRawPassword).toString(),
        };
        if (oldRawPassword) {
            payload.old_password = sha256(oldRawPassword).toString();
        }
        await request.patch(USER_PASSWORD, payload);
        await get().fetch();
    },
    listOAuthLinks: async () => {
        const { data } = await request.get<OAuthLinksResp>(USER_OAUTH_LINKS);
        return data;
    },
    unlinkOAuth: async (provider: string) => {
        await request.delete(
            `${USER_OAUTH_UNLINK}/${encodeURIComponent(provider)}`,
        );
    },
    listMyAuditLogs: async (limit = 15) => {
        const { data } = await request.get<AuditLogItem[]>(USER_AUDIT_LOGS, {
            params: { limit },
        });
        return data;
    },
}));

export default useUserState;
