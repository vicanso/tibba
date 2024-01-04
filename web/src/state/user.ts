import { create } from "zustand";
import request from "@/helpers/request";
import sha256 from "crypto-js/sha256";
import dayjs from "dayjs";
import {
  USER_REFRESH,
  USER_LOGIN,
  USER_LOGIN_TOKEN,
  USER_ME,
  USER_REGISTER,
  USER_LOGOUT,
} from "@/url";

interface UserState {
  anonymous: boolean;
  loading: boolean;
  account: string;
  roles: string[];
  login: (account: string, password: string, captcha: string) => Promise<void>;
  register: (account: string, password: string) => Promise<void>;
  logout: () => void;
  fetch: () => Promise<boolean>;
}

const refresh = (expiredAt: string) => {
  // 如果准备要过期
  const offset = 2 * 24 * 3600;
  if (dayjs(expiredAt).unix() - dayjs().unix() > offset) {
    return;
  }
  request
    .get<{
      access_token: string;
      token_type: string;
    }>(USER_REFRESH)
    .catch(console.error);
};

const useUserStore = create<UserState>()((set, get) => ({
  anonymous: false,
  account: "",
  loading: false,
  roles: [],
  register: async (account: string, password: string) => {
    await request.post(USER_REGISTER, {
      account,
      password: sha256(password).toString(),
    });
  },
  login: async (account: string, password: string, captcha: string) => {
    // 获取token
    const resp = await request.get<{ ts: number; token: string; hash: string }>(
      USER_LOGIN_TOKEN,
    );
    const { token, ts, hash } = resp.data;
    const msg = `${hash}:${sha256(password).toString()}`;
    // 登录
    await request.post<{
      access_token: string;
      token_type: string;
    }>(
      USER_LOGIN,
      {
        token,
        ts,
        hash,
        account,
        password: sha256(msg).toString(),
      },
      {
        headers: {
          "X-Captcha": captcha,
        },
      },
    );
  },
  fetch: async () => {
    if (get().loading) {
      return false;
    }
    set({
      loading: true,
    });
    try {
      const { data } = await request.get<{
        name: string;
        expired_at: string;
        issued_at: string;
        time: string;
        roles: string[];
      }>(USER_ME);
      let account = data.name;
      // 如果超过14天，则认为需要重新登录
      const expiredOffset = 14 * 24 * 3600;
      if (dayjs().unix() - dayjs(data.issued_at).unix() > expiredOffset) {
        account = "";
      }
      set({
        account,
        anonymous: account == "",
        roles: data.roles,
      });
      // 如果已登录，触发刷新ttl
      if (account) {
        refresh(data.expired_at);
      }
    } finally {
      set({
        loading: false,
      });
    }
    return get().account != "";
  },
  logout: async () => {
    await request.delete(USER_LOGOUT);
    set({
      account: "",
      anonymous: true,
    });
  },
}));

export default useUserStore;
