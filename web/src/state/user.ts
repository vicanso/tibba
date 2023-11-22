import { create } from "zustand";
import request, {
  saveAuthorization,
  authorizationExists,
  removeAuthorization,
} from "@/request";
import sha256 from "crypto-js/sha256";
import dayjs from "dayjs";
import HTTPError from "@/http-error";
import { USER_FRESH, USER_LOGIN, USER_LOGIN_TOKEN, USER_ME } from "@/url";

interface UserState {
  anonymous: boolean;
  loading: boolean;
  account: string;
  login: (account: string, password: string) => Promise<void>;
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
    }>(USER_FRESH)
    .then((res) => {
      const { token_type, access_token } = res.data;
      if (access_token) {
        const authorization = `${token_type} ${access_token}`;
        saveAuthorization(authorization);
      }
    })
    .catch(console.error);
};

const useUserStore = create<UserState>()((set, get) => ({
  anonymous: false,
  account: "",
  loading: false,
  login: async (account: string, password: string) => {
    // 获取token
    const resp = await request.get<{ timestamp: number; token: string }>(
      USER_LOGIN_TOKEN,
    );
    const { token, timestamp } = resp.data;
    const msg = `${token}:${sha256(password).toString()}`;
    // 登录
    const { data } = await request.post<{
      access_token: string;
      token_type: string;
    }>(USER_LOGIN, {
      token,
      timestamp,
      account,
      password: sha256(msg).toString(),
    });
    const authorization = `${data.token_type} ${data.access_token}`;
    saveAuthorization(authorization);
  },
  fetch: async () => {
    // 如果不存在，则未登录
    if (!authorizationExists()) {
      set({
        anonymous: true,
      });
      return false;
    }
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
      }>(USER_ME);
      let account = data.name;
      // 如果超过14天，则认为需要重新登录
      const expiredOffset = 14 * 24 * 3600;
      if (dayjs().unix() - dayjs(data.issued_at).unix() > expiredOffset) {
        removeAuthorization();
        account = "";
      }
      set({
        account,
        anonymous: account == "",
      });
      // 如果已登录，触发刷新ttl
      if (account) {
        refresh(data.expired_at);
      }
    } catch (err) {
      const e = err as HTTPError;
      if (e.category !== "jwt") {
        throw err;
      }
      console.error(err);
    } finally {
      set({
        loading: false,
      });
    }
    return get().account != "";
  },
  logout: () => {
    removeAuthorization();
    set({
      account: "",
      anonymous: true,
    });
  },
}));

export default useUserStore;
