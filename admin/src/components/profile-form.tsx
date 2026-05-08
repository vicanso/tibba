"use client";
import { zodResolver } from "@hookform/resolvers/zod";
import { useForm } from "react-hook-form";
import { z } from "zod";
import {
    Form,
    FormControl,
    FormField,
    FormItem,
    FormLabel,
    FormMessage,
} from "@/components/ui/form";
import { toast } from "sonner";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { useShallow } from "zustand/react/shallow";
import useUserState from "@/states/user";
import { Loading } from "@/components/loading";
import { useEffect, useState } from "react";
import { useI18n } from "@/i18n";
import { formatError, getDirtyValues, DirtyFields } from "@/helpers/util";
import { Loader2Icon } from "lucide-react";

const profileFormSchema = z.object({
    email: z.email({ message: "Please input your email." }),
    avatar: z.url({ message: "Please enter a valid URL." }),
    roles: z.array(z.string()).optional(),
    groups: z.array(z.string()).optional(),
});

type ProfileFormValues = z.infer<typeof profileFormSchema>;

export function ProfileForm() {
    const profileI18n = useI18n("profile");

    const [processing, setProcessing] = useState(false);

    const [user, initialized, updateProfile] = useUserState(
        useShallow((state) => [
            state.data,
            state.initialized,
            state.updateProfile,
        ]),
    );
    const form = useForm<ProfileFormValues>({
        resolver: zodResolver(profileFormSchema),
        defaultValues: {
            email: user.email ?? "",
            avatar: user.avatar ?? "",
            roles: user.roles ?? [],
            groups: user.groups ?? [],
        },
        mode: "onChange",
    });
    useEffect(() => {
        if (initialized) {
            form.reset({
                email: user.email ?? "",
                avatar: user.avatar ?? "",
                roles: user.roles ?? [],
                groups: user.groups ?? [],
            });
        }
    }, [initialized, user, form]);

    async function onSubmit(data: ProfileFormValues) {
        if (processing || !form.formState.isDirty) {
            return;
        }
        setProcessing(true);
        try {
            const dirtyValues = getDirtyValues(
                form.formState.dirtyFields as DirtyFields<ProfileFormValues>,
                data,
            );
            await updateProfile(dirtyValues);
            toast.success(profileI18n("updateSuccess"));
        } catch (error) {
            toast.error(formatError(error));
        } finally {
            setProcessing(false);
        }
    }

    if (!initialized) {
        return <Loading />;
    }

    return (
        <Form {...form}>
            <form
                onSubmit={form.handleSubmit(onSubmit)}
                className="grid grid-cols-2 gap-4"
            >
                <FormField
                    control={form.control}
                    name="email"
                    render={({ field }) => (
                        <FormItem>
                            <FormLabel>{profileI18n("email")}</FormLabel>
                            <FormControl>
                                <Input
                                    placeholder={profileI18n(
                                        "emailPlaceholder",
                                    )}
                                    {...field}
                                />
                            </FormControl>
                            <FormMessage />
                        </FormItem>
                    )}
                />
                <FormField
                    control={form.control}
                    name="avatar"
                    render={({ field }) => (
                        <FormItem>
                            <FormLabel>{profileI18n("avatar")}</FormLabel>
                            <FormControl>
                                <Input
                                    placeholder={profileI18n(
                                        "avatarPlaceholder",
                                    )}
                                    {...field}
                                />
                            </FormControl>
                            <FormMessage />
                        </FormItem>
                    )}
                />
                <FormField
                    control={form.control}
                    name="roles"
                    render={({ field }) => (
                        <FormItem>
                            <FormLabel>{profileI18n("roles")}</FormLabel>
                            <FormControl>
                                <Input
                                    disabled={true}
                                    value={field.value?.join(", ")}
                                />
                            </FormControl>
                            <FormMessage />
                        </FormItem>
                    )}
                />
                <FormField
                    control={form.control}
                    name="groups"
                    render={({ field }) => (
                        <FormItem>
                            <FormLabel>{profileI18n("groups")}</FormLabel>
                            <FormControl>
                                <Input
                                    disabled={true}
                                    value={field.value?.join(", ")}
                                />
                            </FormControl>
                            <FormMessage />
                        </FormItem>
                    )}
                />

                <Button type="submit" disabled={!form.formState.isDirty}>
                    {processing && (
                        <Loader2Icon className="size-4 animate-spin mr-2" />
                    )}
                    {profileI18n("submit")}
                </Button>
            </form>
        </Form>
    );
}
