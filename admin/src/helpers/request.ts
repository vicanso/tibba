import axios, { AxiosRequestConfig, AxiosResponse } from "axios";
import HTTPError from "@/helpers/http-error";

const requestedAt = "X-Requested-At";
const csrfCookieName = "csrf_token";
const csrfHeaderName = "X-CSRF-Token";
// 安全方法不改状态，无需 CSRF token
const csrfSafeMethods = ["get", "head", "options"];

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

// 确保已持有 CSRF token：cookie 缺失时先向后端拉取一次（该 GET 会种下 cookie）
async function ensureCsrfToken(): Promise<string> {
    const existing = readCsrfCookie();
    if (existing) {
        return existing;
    }
    // 直接用底层 axios（绕过本拦截器的 /api 前缀逻辑），GET 幂等且不需 CSRF
    const { data } = await axios.get<{ token: string }>("/api/csrf/token");
    return data?.token || readCsrfCookie();
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
        config.url = `/api${config.url}`;
        if (config.headers) {
            config.headers[requestedAt] = `${Date.now()}`;
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
    (err) => {
        const { response } = err;
        const he = new HTTPError(0, "请求出错");
        if (timeoutErrorCodes.includes(err.code)) {
            he.exception = true;
            he.code = err.code;
            he.category = "timeout";
            he.message = "请求超时，请稍候再试";
        } else if (response) {
            he.status = response.status;
            if (response.data && response.data.message) {
                he.message = response.data.message;
                he.code = response.data.code;
                he.category = response.data.category;
            } else {
                he.exception = true;
                he.category = "exception";
                he.message = `未知错误`;
            }
            he.extra = response.data?.extra;
        }
        addRequestStats(response?.config, response, he);
        return Promise.reject(he);
    },
);

export default request;
