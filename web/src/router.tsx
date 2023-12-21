import { createHashRouter } from "react-router-dom";

import Home from "@/pages/Home";
import LoginRegister from "@/pages/LoginRegister";
import Entity from "@/pages/Entity";
import EntityEditor from "@/pages/EntityEditor";
import { LOGIN, HOME, ENTITY, ENTITY_EDITOR } from "@/data/route";

export type NavItemList = (typeof navItemList)[number];

export const navItemList = [
  {
    name: "用户",
    items: [
      {
        name: "用户列表",
        url: ENTITY.replace(":entity", "users"),
      },
    ],
  },
  {
    name: "配置",
    items: [
      {
        name: "应用配置",
        url: ENTITY.replace(":entity", "settings"),
      },
    ],
  },
];

const router = createHashRouter([
  {
    path: HOME,
    element: <Home />,
  },
  {
    path: LOGIN,
    element: <LoginRegister />,
  },
  {
    path: ENTITY,
    element: <Entity />,
  },
  {
    path: ENTITY_EDITOR,
    element: <EntityEditor />,
  },
]);

export function goToLogin() {
  router.navigate(LOGIN);
}

export function goBack() {
  router.navigate(-1);
}

export function goToEntityForm(entity: string, id: string) {
  const url = ENTITY_EDITOR.replace(":entity", entity).replace(":id", id);
  router.navigate(url);
}

export default router;
