import { createHashRouter } from "react-router";
import Root from "@/pages/Root";
import Home from "@/pages/Home";
import LoginHistory from "@/pages/LoginHistory";
import Login from "@/pages/Login";
import SignUp from "@/pages/SignUp";
import Profile from "@/pages/Profile";
import Model from "@/pages/Model";
import ModelEditor from "@/pages/ModelEditor";
import FileUploader from "@/pages/FileUploader";

import {
    LOGIN_HISTORY,
    HOME,
    LOGIN,
    SIGN_UP,
    PROFILE,
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
                path: LOGIN_HISTORY,
                element: <LoginHistory />,
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
                path: PROFILE,
                element: <Profile />,
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
