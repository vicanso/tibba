import axios, {
    AxiosRequestConfig,
    AxiosResponse,
    InternalAxiosRequestConfig,
} from "axios";
import HTTPError from "@/helpers/http-error";
import i18n from "@/i18n";

const requestedAt = "X-Requested-At";
const csrfCookieName = "csrf_token";
const csrfHeaderName = "X-CSRF-Token";
// 安全方法不改状态，无需 CSRF token
const csrfSafeMethods = ["get", "head", "options"];
const csrfSubCategories = new Set([
    "csrf_cookie_missing",
    "csrf_header_missing",
    "csrf_mismatch",
]);

interface RetryableRequestConfig extends InternalAxiosRequestConfig {
    _csrfRetried?: boolean;
}

const request = axios.create({
    // 默认超时为10秒
    timeout: 10 * 1000,
});

// 从 document.cookie 读取非 HttpOnly 的 csrf_token（double-submit 模式）
function readCsrfCookie(): string {
    const m = document.cookie.match(
        new RegExp(`(?:^|;\\s*)${csrfCookieName}=([^;]+)`),
    );
    return m ? decodeURIComponent(m[1]) : "";
}

let csrfPromise: Promise<string> | null = null;

// 确保已持有 CSRF token：cookie 缺失或强制刷新时向后端拉取一次（单飞去重）
async function ensureCsrfToken(forceRefresh = false): Promise<string> {
    if (!forceRefresh) {
        const existing = readCsrfCookie();
        if (existing) {
            return existing;
        }
    }
    if (!csrfPromise) {
        // 直接用底层 axios（绕过本拦截器的 /api 前缀逻辑），GET 幂等且不需 CSRF
        csrfPromise = axios
            .get<{ token: string }>("/api/csrf/token")
            .then(({ data }) => data?.token || readCsrfCookie())
            .finally(() => {
                csrfPromise = null;
            });
    }
    return csrfPromise;
}

request.interceptors.request.use(
    async (config) => {
        // 对请求的query部分清空值
        if (config.params) {
            Object.keys(config.params).forEach((element) => {
                // 空字符
                if (config.params[element] === "") {
                    delete config.params[element];
                }
            });
        }
        if (config.url && !config.url.startsWith("/api/")) {
            config.url = `/api${config.url}`;
        }
        if (config.headers) {
            config.headers[requestedAt] = `${Date.now()}`;
            config.headers["Accept-Language"] = i18n.language || "zh";
            // 状态变更请求附带 CSRF token，与后端 validate_csrf 的 double-submit 校验对齐
            const method = (config.method || "get").toLowerCase();
            if (!csrfSafeMethods.includes(method)) {
                config.headers[csrfHeaderName] = await ensureCsrfToken();
            }
        }
        return config;
    },
    (err) => {
        return Promise.reject(err);
    },
);

// addRequestStats 添加http请求的相关记录
function addRequestStats(
    config: AxiosRequestConfig | undefined,
    res: AxiosResponse | undefined,
    he: HTTPError | undefined,
): void {
    const data: Record<string, unknown> = {};
    if (config) {
        data.method = config.method;
        data.url = config.url;
        data.data = config.data;
        if (config.headers) {
            const value = config.headers[requestedAt];
            data.use = Date.now() - Number(value);
        }
    }
    if (res) {
        data.status = res.status;
    }
    if (he) {
        data.message = he.message;
    }
    // httpRequests.add(data);
}

// 设置接口最少要x ms才完成，能让客户看到loading
const minUse = 300;
const timeoutErrorCodes = ["ECONNABORTED", "ECONNREFUSED", "ECONNRESET"];
request.interceptors.response.use(
    async (res) => {
        addRequestStats(res.config, res, undefined);
        // 根据请求开始时间计算耗时，并判断是否需要延时响应
        if (res.config.method != "get" && res.config.headers) {
            const value = res.config.headers[requestedAt];
            if (value) {
                const use = Date.now() - Number(value);
                if (use >= 0 && use < minUse) {
                    await new Promise((resolve) =>
                        setTimeout(resolve, minUse - use),
                    );
                }
            }
        }
        return res;
    },
    async (err) => {
        const { response } = err;
        const config = err.config as RetryableRequestConfig | undefined;
        const subCategory = response?.data?.sub_category as string | undefined;
        if (
            response?.status === 403 &&
            subCategory &&
            csrfSubCategories.has(subCategory) &&
            config &&
            !config._csrfRetried
        ) {
            config._csrfRetried = true;
            const freshToken = await ensureCsrfToken(true);
            if (config.headers) {
                config.headers[csrfHeaderName] = freshToken;
            }
            return request(config);
        }

        const he = new HTTPError(0, i18n.t("common.requestFailed"));
        if (timeoutErrorCodes.includes(err.code)) {
            he.exception = true;
            he.code = err.code;
            he.category = "timeout";
            he.message = i18n.t("common.requestTimeout");
        } else if (response) {
            he.status = response.status;
            if (response.data && response.data.message) {
                he.message = response.data.message;
                he.code = response.data.code;
                he.category = response.data.category;
                he.subCategory = response.data.sub_category;
            } else {
                he.exception = true;
                he.category = "exception";
                he.message = i18n.t("common.unknownError");
            }
            he.extra = response.data?.extra;
        }
        addRequestStats(response?.config, response, he);
        return Promise.reject(he);
    },
);

export default request;
