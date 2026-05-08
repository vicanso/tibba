import useBasicState from "@/states/basic";
import { useShallow } from "zustand/react/shallow";
import { useEffect } from "react";
export default function Home() {
    const [setPageHeaderNavigation, resetPageHeaderNavigation] = useBasicState(
        useShallow((state) => [
            state.setPageHeaderNavigation,
            state.resetPageHeaderNavigation,
        ]),
    );
    useEffect(() => {
        setPageHeaderNavigation([
            {
                title: "Home",
                url: "/",
            },
            {
                title: "Login",
                url: "/login",
            },
            {
                title: "Sign Up",
                url: "/sign-up",
            },
        ]);
        return () => {
            resetPageHeaderNavigation();
        };
    });
    return (
        <div>
            <h1>Home</h1>
        </div>
    );
}
