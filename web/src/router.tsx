import { createHashRouter } from "react-router-dom";

import App from "@/App.tsx";

export type NavItemList = (typeof navItemList)[number];

export const navItemList = [
  {
    name: "用户",
    items: [
      {
        name: "用户列表",
        url: "/",
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
    path: "/",
    element: <App />,
  },
  {
    path: "/users",
    element: <App />,
  }
]);

export default router;
