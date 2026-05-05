import { create } from "zustand";
import request from "@/helpers/request";
import { COMMON_CAPTCHA } from "@/constants/url";

interface Captcha {
    id: string;
    data: string;
}

interface CommonState {
    fetchCaptcha: (theme: string) => Promise<Captcha>;
}

const useCommonState = create<CommonState>(() => ({
    fetchCaptcha: async (theme: string) => {
        const { data } = await request.get<Captcha>(COMMON_CAPTCHA, {
            params: {
                theme,
            },
        });
        return data;
    },
}));

export default useCommonState;
