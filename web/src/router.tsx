import { createHashRouter } from "react-router-dom";

import Home from "@/pages/Home";
import LoginRegister from "@/pages/LoginRegister";
import Entity from "@/pages/Entity";
import { LOGIN, HOME, ENTITY } from "@/data/route";

export type NavItemList = (typeof navItemList)[number];

export const navItemList = [
  {
    name: "用户",
    items: [
      {
        name: "用户列表",
        url: HOME,
      },
      {
        name: "登录记录",
        url: "/users",
      },
    ],
  },
  {
    name: "配置",
    items: [
      {
        name: "应用配置",
        url: "/entity/settings",
      },
      {
        name: "用户",
        url: "/entity/users",
      },
    ],
  },
];

const router = createHashRouter([
  {
    path: HOME,
    element: <Home />,
  },
  // {
  //   path: "/users",
  //   element:  <Home />,
  // },
  {
    path: LOGIN,
    element: <LoginRegister />,
  },
  {
    path: ENTITY,
    element: <Entity />,
  },
]);

export function goToLogin() {
  router.navigate(LOGIN);
}

export function goBack() {
  router.navigate(-1);
}

export default router;
