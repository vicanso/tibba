"use client";

import { Button } from "@/components/ui/button";
import {
    FileUpload,
    FileUploadDropzone,
    FileUploadItem,
    FileUploadItemDelete,
    FileUploadItemMetadata,
    FileUploadItemPreview,
    FileUploadList,
    FileUploadTrigger,
} from "@/components/ui/file-upload";
import { Label } from "@/components/ui/label";
import {
    Select,
    SelectContent,
    SelectItem,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { FILE_UPLOAD } from "@/constants/url";
import { useI18n } from "@/i18n";
import { Upload, X, Loader2Icon } from "lucide-react";
import * as React from "react";
import { useSearchParams } from "react-router";
import { toast } from "sonner";
import request from "@/helpers/request";
import { formatError } from "@/helpers/util";

export default function FileUploader() {
    const i18n = useI18n("component");
    const [searchParams] = useSearchParams();
    const [files, setFiles] = React.useState<File[]>([]);
    const [group, setGroup] = React.useState<string>("");
    const [processing, setProcessing] = React.useState<boolean>(false);

    const groups = searchParams.get("groups")?.split(",") || [];

    const onFileReject = React.useCallback((file: File, message: string) => {
        toast(message, {
            description: `"${file.name.length > 20 ? `${file.name.slice(0, 20)}...` : file.name}" has been rejected`,
        });
    }, []);

    const handleUpload = async () => {
        if (processing) {
            return;
        }
        setProcessing(true);
        try {
            await request.postForm(`${FILE_UPLOAD}?group=${group}`, {
                "files[]": files,
            });
            toast.success(i18n("uploadedSuccessfully"));
            setFiles([]);
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setProcessing(false);
        }
    };

    return (
        <div className="grid grid-cols-2 gap-4">
            <div className="col-span-1">
                <Label className="mb-2">{i18n("group")}</Label>
                <Select onValueChange={(value) => setGroup(value)}>
                    <SelectTrigger className="w-full">
                        <SelectValue placeholder={i18n("selectGroup")} />
                    </SelectTrigger>
                    <SelectContent className="w-full">
                        {groups.map((group) => (
                            <SelectItem key={group} value={group}>
                                {group}
                            </SelectItem>
                        ))}
                    </SelectContent>
                </Select>
            </div>
            <div className="col-span-1">
                <Label className="mb-2">{i18n("upload")}</Label>
                <Button
                    variant="outline"
                    className="w-full"
                    disabled={group === "" || files.length === 0 || processing}
                    onClick={handleUpload}
                >
                    {processing && (
                        <Loader2Icon className="size-4 animate-spin" />
                    )}
                    {i18n("upload")}
                </Button>
            </div>

            <div className="col-span-2">
                <Label className="mb-2">{i18n("files")}</Label>
                <FileUpload
                    maxFiles={10}
                    maxSize={5 * 1024 * 1024}
                    className="w-full"
                    value={files}
                    onValueChange={setFiles}
                    onFileReject={onFileReject}
                    multiple
                >
                    <FileUploadDropzone>
                        <div className="flex flex-col items-center gap-1">
                            <div className="flex items-center justify-center rounded-full border p-2.5">
                                <Upload className="size-6 text-muted-foreground" />
                            </div>
                            <p className="font-medium text-sm">
                                {i18n("dragUpload")}
                            </p>
                            <p className="text-muted-foreground text-xs">
                                {i18n("clickUpload")}
                            </p>
                        </div>
                        <FileUploadTrigger asChild>
                            <Button
                                variant="outline"
                                size="sm"
                                className="mt-2 w-fit"
                            >
                                {i18n("browseFiles")}
                            </Button>
                        </FileUploadTrigger>
                    </FileUploadDropzone>
                    <FileUploadList className="grid grid-cols-2">
                        {files.map((file, index) => (
                            <FileUploadItem
                                className="col-span-1"
                                key={index}
                                value={file}
                            >
                                <FileUploadItemPreview />
                                <FileUploadItemMetadata />
                                <FileUploadItemDelete asChild>
                                    <Button
                                        variant="ghost"
                                        size="icon"
                                        className="size-7"
                                    >
                                        <X />
                                    </Button>
                                </FileUploadItemDelete>
                            </FileUploadItem>
                        ))}
                    </FileUploadList>
                </FileUpload>
            </div>
        </div>
    );
}
