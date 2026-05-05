import HTTPError from "@/helpers/http-error";
import dayjs from "dayjs";

const oneHourMS = 3600 * 1000;
const oneDayMS = 24 * oneHourMS;

// formatDate 格式化日期
export function formatDate(str: string): string {
    if (!str) {
        return "--";
    }

    return dayjs(str).format("YYYY-MM-DD HH:mm:ss");
}

export function isError(err: Error | HTTPError | unknown, category: string) {
    if (err instanceof HTTPError) {
        return err.category === category;
    }
    return false;
}

export function isErrorCode(err: Error | HTTPError | unknown, code: string) {
    if (err instanceof HTTPError) {
        return err.code === code;
    }
    return false;
}

export function formatError(err: Error | HTTPError | unknown): string {
    let message = "";
    if (err instanceof HTTPError) {
        message = err.message;
        if (err.category) {
            message += ` [${err.category.toUpperCase()}]`;
        }
        if (err.code) {
            message += ` [${err.code}]`;
        }
        // 如果是异常（客户端异常，如请求超时，中断等），则上报user action
        if (err.exception) {
            // const currentLocation = getCurrentLocation();
            // actionAdd({
            //   category: ERROR,
            //   route: currentLocation.name,
            //   path: currentLocation.path,
            //   result: FAIL,
            //   message,
            // });
        }
    } else if (err instanceof Error) {
        message = err.message;
    } else {
        message = (err as Error).message;
    }
    return message;
}

// today 获取当天0点时间
export function today(): Date {
    return new Date(new Date(new Date().toLocaleDateString()).getTime());
}

// tomorrow 获取明天0点时间
export function tomorrow(): Date {
    return new Date(today().getTime() + oneDayMS);
}

export type DirtyFields<T> = {
    [K in keyof T]?: boolean;
};

export function getDirtyValues<T extends object>(
    dirtyFields: DirtyFields<T>,
    allValues: T,
): Partial<T> {
    return Object.keys(dirtyFields).reduce((acc, key) => {
        const typedKey = key as keyof T;
        acc[typedKey] = allValues[typedKey];
        return acc;
    }, {} as Partial<T>);
}

export function getSystemTheme() {
    if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
        return "dark";
    } else {
        return "light"; // 默认浅色
    }
}
