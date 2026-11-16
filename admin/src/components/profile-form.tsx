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
import { Badge } from "@/components/ui/badge";
import { Label } from "@/components/ui/label";
import { useShallow } from "zustand/react/shallow";
import useUserState from "@/states/user";
import { Loading } from "@/components/loading";
import { useEffect, useMemo, useState } from "react";
import { useI18n } from "@/i18n";
import { formatError, getDirtyValues, DirtyFields } from "@/helpers/util";
import { Loader2Icon } from "lucide-react";

interface ProfileFormValues {
    nickname: string;
    phone: string;
    email: string;
    avatar: string;
}

export function ProfileForm() {
    const profileI18n = useI18n("profile");
    const [processing, setProcessing] = useState(false);

    const profileFormSchema = useMemo(
        () =>
            z.object({
                nickname: z.string().max(100),
                phone: z.string().max(64),
                email: z.union([
                    z.email({ message: profileI18n("invalidEmail") }),
                    z.literal(""),
                ]),
                avatar: z.union([
                    z.url({ message: profileI18n("invalidAvatarUrl") }),
                    z.literal(""),
                ]),
            }),
        [profileI18n],
    );

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
            nickname: user.nickname ?? "",
            phone: user.phone ?? "",
            email: user.email ?? "",
            avatar: user.avatar ?? "",
        },
        mode: "onChange",
    });
    useEffect(() => {
        if (initialized) {
            form.reset({
                nickname: user.nickname ?? "",
                phone: user.phone ?? "",
                email: user.email ?? "",
                avatar: user.avatar ?? "",
            });
        }
    }, [initialized, user, form]);

    async function onSubmit(data: ProfileFormValues) {
        if (processing || !form.formState.isDirty) {
            return;
        }
        const dirtyValues = getDirtyValues(
            form.formState.dirtyFields as DirtyFields<ProfileFormValues>,
            data,
        );
        const payload: {
            nickname?: string;
            phone?: string;
            email?: string;
            avatar?: string;
        } = {};
        if (dirtyValues.nickname !== undefined) {
            payload.nickname = dirtyValues.nickname.trim();
        }
        if (dirtyValues.phone !== undefined) {
            payload.phone = dirtyValues.phone.trim();
        }
        if (dirtyValues.email !== undefined && dirtyValues.email.trim()) {
            payload.email = dirtyValues.email.trim();
        }
        if (dirtyValues.avatar !== undefined && dirtyValues.avatar.trim()) {
            payload.avatar = dirtyValues.avatar.trim();
        }
        if (Object.keys(payload).length === 0) {
            toast.info(profileI18n("noChange"));
            return;
        }

        setProcessing(true);
        try {
            await updateProfile(payload);
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
        <div className="space-y-6">
            <div className="grid gap-4 sm:grid-cols-2">
                <div className="space-y-1.5">
                    <Label>{profileI18n("account")}</Label>
                    <Input disabled value={user.account} />
                </div>
                <div className="space-y-1.5">
                    <Label>{profileI18n("roles")}</Label>
                    <div className="flex min-h-9 flex-wrap items-center gap-1.5 rounded-md border px-3 py-1.5">
                        {user.roles.length > 0 ? (
                            user.roles.map((role) => (
                                <Badge key={role} variant="secondary">
                                    {role}
                                </Badge>
                            ))
                        ) : (
                            <span className="text-sm text-muted-foreground">
                                -
                            </span>
                        )}
                    </div>
                </div>
                <div className="space-y-1.5">
                    <Label>{profileI18n("groups")}</Label>
                    <div className="flex min-h-9 flex-wrap items-center gap-1.5 rounded-md border px-3 py-1.5">
                        {user.groups.length > 0 ? (
                            user.groups.map((group) => (
                                <Badge key={group} variant="outline">
                                    {group}
                                </Badge>
                            ))
                        ) : (
                            <span className="text-sm text-muted-foreground">
                                -
                            </span>
                        )}
                    </div>
                </div>
                <div className="space-y-1.5">
                    <Label>{profileI18n("permissions")}</Label>
                    <div className="flex min-h-9 flex-wrap items-center gap-1.5 rounded-md border px-3 py-1.5">
                        {user.permissions.length > 0 ? (
                            user.permissions.map((perm) => (
                                <Badge
                                    key={perm}
                                    variant="outline"
                                    className="font-mono text-xs"
                                >
                                    {perm}
                                </Badge>
                            ))
                        ) : (
                            <span className="text-sm text-muted-foreground">
                                -
                            </span>
                        )}
                    </div>
                </div>
            </div>

            <Form {...form}>
                <form
                    onSubmit={form.handleSubmit(onSubmit)}
                    className="grid gap-4 sm:grid-cols-2"
                >
                    <FormField
                        control={form.control}
                        name="nickname"
                        render={({ field }) => (
                            <FormItem>
                                <FormLabel>{profileI18n("nickname")}</FormLabel>
                                <FormControl>
                                    <Input
                                        placeholder={profileI18n(
                                            "nicknamePlaceholder",
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
                        name="phone"
                        render={({ field }) => (
                            <FormItem>
                                <FormLabel>{profileI18n("phone")}</FormLabel>
                                <FormControl>
                                    <Input
                                        placeholder={profileI18n(
                                            "phonePlaceholder",
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

                    <div className="sm:col-span-2">
                        <Button
                            type="submit"
                            disabled={!form.formState.isDirty || processing}
                        >
                            {processing && (
                                <Loader2Icon className="mr-2 size-4 animate-spin" />
                            )}
                            {profileI18n("submit")}
                        </Button>
                    </div>
                </form>
            </Form>
        </div>
    );
}
