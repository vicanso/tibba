import { createHashRouter } from "react-router-dom";

import App from "@/App.tsx";
import Login from "@/pages/Login";
import { LOGIN, HOME } from "@/data/route";

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
        url: "/settings",
      },
    ],
  },
];

const router = createHashRouter([
  {
    path: HOME,
    element: <App />,
  },
  {
    path: "/users",
    element: <App />,
  },
  {
    path: LOGIN,
    element: <Login />,
  },
]);

export default router;
