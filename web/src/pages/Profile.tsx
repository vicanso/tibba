import { ProfileForm } from "@/components/profile-form";
import { useI18n } from "@/i18n";
import { useEffect } from "react";
import { useShallow } from "zustand/react/shallow";
import useBasicState from "@/states/basic";
import { UserIcon } from "lucide-react";

export default function Profile() {
    const profileI18n = useI18n("profile");
    const [setPageHeaderNavigation] = useBasicState(
        useShallow((state) => [state.setPageHeaderNavigation]),
    );
    useEffect(() => {
        setPageHeaderNavigation([
            {
                title: profileI18n("title"),
                icon: <UserIcon className="size-4" />,
            },
        ]);
    });

    return (
        <div className="flex w-full items-center justify-center p-6">
            <div className="w-full">
                <ProfileForm />
            </div>
        </div>
    );
}
