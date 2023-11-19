import { create } from "zustand";
import request, {
  saveAuthorization,
  authorizationExists,
  removeAuthorization,
} from "@/request";
import sha256 from "crypto-js/sha256";
import dayjs from "dayjs";

interface UserState {
  anonymous: boolean;
  loading: boolean;
  account: string;
  login: (account: string, password: string) => Promise<void>;
  fetch: () => Promise<boolean>;
}

const useUserStore = create<UserState>()((set, get) => ({
  anonymous: false,
  account: "",
  loading: false,
  login: async (account: string, password: string) => {
    // 获取token
    const resp = await request.get<{ timestamp: number; token: string }>(
      "/users/login-token",
    );
    const { token, timestamp } = resp.data;
    const msg = `${token}:${sha256(password).toString()}`;
    // 登录
    const { data } = await request.post<{
      access_token: string;
      token_type: string;
    }>("/users/login", {
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
      }>("/users/me");
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
      return true;
    } finally {
      set({
        loading: false,
      });
    }
    return false;
  },
}));

export default useUserStore;
