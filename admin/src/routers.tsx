import { createHashRouter } from "react-router";
import Root from "@/pages/Root";
import Home from "@/pages/Home";
import Login from "@/pages/Login";
import SignUp from "@/pages/SignUp";
import ResetPassword from "@/pages/ResetPassword";
import Profile from "@/pages/Profile";
import Model from "@/pages/Model";
import ModelEditor from "@/pages/ModelEditor";
import FileUploader from "@/pages/FileUploader";
import Jobs from "@/pages/Jobs";
import Rbac from "@/pages/Rbac";
import AuditLogs from "@/pages/AuditLogs";
import SqlQueries from "@/pages/SqlQueries";
import Apps from "@/pages/Apps";

import {
    HOME,
    LOGIN,
    SIGN_UP,
    RESET_PASSWORD,
    PROFILE,
    JOBS,
    RBAC,
    AUDIT_LOGS,
    SQL_QUERIES,
    APPS,
    MODEL,
    MODEL_EDITOR,
    FILE_UPLOADER,
} from "@/constants/route";

const router = createHashRouter([
    {
        path: "/",
        element: <Root />,
        children: [
            {
                path: HOME,
                element: <Home />,
            },
            {
                path: LOGIN,
                element: <Login />,
            },
            {
                path: SIGN_UP,
                element: <SignUp />,
            },
            {
                path: RESET_PASSWORD,
                element: <ResetPassword />,
            },
            {
                path: PROFILE,
                element: <Profile />,
            },
            {
                path: JOBS,
                element: <Jobs />,
            },
            {
                path: RBAC,
                element: <Rbac />,
            },
            {
                path: AUDIT_LOGS,
                element: <AuditLogs />,
            },
            {
                path: SQL_QUERIES,
                element: <SqlQueries />,
            },
            {
                path: APPS,
                element: <Apps />,
            },
            {
                path: `${MODEL}/:name`,
                element: <Model />,
            },
            {
                path: `${MODEL_EDITOR}/:name/:id`,
                element: <ModelEditor />,
            },
            {
                path: `${FILE_UPLOADER}`,
                element: <FileUploader />,
            },
        ],
    },
]);

export function goTo(path: string) {
    router.navigate(path);
}

export function goBack() {
    router.navigate(-1);
}

export default router;
