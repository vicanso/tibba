import { Button } from "@/components/ui/button";
import { formatError } from "@/helpers/util";
import { useI18n } from "@/i18n";
import { goBack } from "@/routers";
import useModelState, { Category, SchemaView } from "@/states/model";
import { useParams, useSearchParams } from "react-router";
import { useShallow } from "zustand/react/shallow";
import { useAsync } from "react-async-hook";
import { toast } from "sonner";
import { Input } from "@/components/ui/input";
import React, { useState } from "react";
import { Loading } from "@/components/loading";
import { MultiSelect } from "@/components/multi-select";
import { Badge } from "@/components/ui/badge";
import {
    ResultBadge,
    StatusBadge,
    StatusRadioGroup,
} from "@/components/model-components";
import { zodResolver } from "@hookform/resolvers/zod";
import { Loader2Icon } from "lucide-react";
import * as z from "zod";
import { useForm, ControllerRenderProps } from "react-hook-form";
import { Switch } from "@/components/ui/switch";
import {
    Form,
    FormControl,
    FormField,
    FormItem,
    FormLabel,
} from "@/components/ui/form";
import dayjs from "dayjs";
import { DateTimePicker } from "@/components/date-time-picker";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import {
    Select,
    SelectContent,
    SelectGroup,
    SelectItem,
    SelectLabel,
    SelectTrigger,
    SelectValue,
} from "@/components/ui/select";
import { isNil, isObjectLike, toString } from "lodash-es";
interface SearchOption {
    label: string;
    value: string | number;
}

const SearchSelect = React.forwardRef<
    HTMLInputElement,
    {
        value: string;
        disabled?: boolean;
        searchModel: string;
        onSearch: (model: string, keyword: string) => Promise<{ options: SearchOption[] }>;
        onChange: (value: string) => void;
        onBlur?: () => void;
    }
>(function SearchSelect({ value, disabled, searchModel, onSearch, onChange, onBlur }, ref) {
    const [inputValue, setInputValue] = React.useState("");
    const [options, setOptions] = React.useState<SearchOption[]>([]);
    const [showList, setShowList] = React.useState(false);
    const timerRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);
    const selectedLabel = React.useRef("");

    React.useEffect(() => {
        if (!value) {
            selectedLabel.current = "";
            setInputValue("");
        }
    }, [value]);

    function handleChange(e: React.ChangeEvent<HTMLInputElement>) {
        const keyword = e.target.value;
        setInputValue(keyword);
        setShowList(true);
        if (timerRef.current) clearTimeout(timerRef.current);
        timerRef.current = setTimeout(async () => {
            const result = await onSearch(searchModel, keyword);
            setOptions(result.options);
        }, 300);
    }

    function handleSelect(option: SearchOption) {
        selectedLabel.current = option.label;
        setInputValue(option.label);
        setOptions([]);
        setShowList(false);
        onChange(String(option.value));
    }

    function handleBlur() {
        setShowList(false);
        setInputValue(selectedLabel.current);
        onBlur?.();
    }

    return (
        <div className="relative">
            <Input
                ref={ref}
                value={inputValue}
                disabled={disabled}
                onChange={handleChange}
                onBlur={handleBlur}
                onFocus={() => setShowList(options.length > 0)}
            />
            {showList && options.length > 0 && (
                <ul className="absolute z-50 mt-1 w-full rounded-md border bg-popover shadow-md">
                    {options.map((option) => (
                        <li
                            key={option.value}
                            className="cursor-pointer px-3 py-2 text-sm hover:bg-accent"
                            onMouseDown={(e) => {
                                e.preventDefault();
                                handleSelect(option);
                            }}
                        >
                            {option.label}
                        </li>
                    ))}
                </ul>
            )}
        </div>
    );
});

enum EditType {
    Edit = "edit",
    Create = "create",
    View = "view",
}

export default function ModelEditor() {
    const i18nModelEditor = useI18n("modelEditor");
    const i18nModel = useI18n("model");
    const i18nComponent = useI18n("component");
    const [searchParams] = useSearchParams();
    const { name: modelName, id } = useParams();
    const [initialized, setInitialized] = useState(false);
    const [processing, setProcessing] = useState(false);
    const [detail, setDetail] = useState<Record<string, unknown>>({});
    const [schemaView, fetchSchema, modelDetail, modelUpdate, modelCreate, modelSearch] =
        useModelState(
            useShallow((state) => [
                state.schemaView,
                state.fetchSchema,
                state.detail,
                state.update,
                state.create,
                state.search,
            ]),
        );
    const modelId = Number(id);
    let editType = EditType.View;
    if (searchParams.get("type") === "edit") {
        if (modelId === 0) {
            editType = EditType.Create;
        } else {
            editType = EditType.Edit;
        }
    }
    const zodSchema: Record<string, z.ZodTypeAny> = {};
    schemaView.schemas?.forEach((item) => {
        const { name, category, read_only, required } = item;
        if (editType === EditType.Create && item.auto_create) {
            return;
        }
        if (editType !== EditType.Create && read_only) {
            return;
        }

        switch (category) {
            case Category.Strings: {
                zodSchema[name] = z.array(z.string());
                break;
            }
            case Category.Status: {
                zodSchema[name] = z.number().min(0).max(1);
                break;
            }
            case Category.Boolean: {
                zodSchema[name] = z.boolean();
                break;
            }
            case Category.Number: {
                zodSchema[name] = z.number().min(0);
                break;
            }
            default:
                zodSchema[name] = z.string();
                break;
        }
        if (!required) {
            zodSchema[name] = zodSchema[name].optional();
        }
    });
    const form = useForm({
        resolver: zodResolver(z.object(zodSchema)),
        defaultValues: detail,
        mode: "onChange",
    });

    function formatValue(values: Record<string, unknown>) {
        const categoryDict: Record<string, Category> = {};
        schemaView.schemas?.forEach((schema) => {
            const { name, category } = schema;
            categoryDict[name] = category;
        });
        const data: Record<string, unknown> = {};

        Object.keys(values).forEach((key) => {
            const value = values[key];
            if (isNil(value)) {
                return;
            }
            switch (categoryDict[key]) {
                case Category.Json: {
                    if (value) {
                        data[key] = JSON.parse(value as string);
                    }
                    break;
                }
                case Category.Number: {
                    data[key] = Number(value);
                    break;
                }
                default:
                    data[key] = value;
                    break;
            }
        });
        return data;
    }

    function formatDefaultValue(schemaView: SchemaView) {
        const data: Record<string, unknown> = {};
        schemaView.schemas?.forEach((schema) => {
            const { name, default_value } = schema;
            if (!isNil(default_value)) {
                data[name] = default_value;
            }
        });
        return data;
    }

    function formatFormValue(
        schemaView: SchemaView,
        values: Record<string, unknown>,
    ) {
        const categoryDict: Record<string, Category> = {};
        schemaView.schemas?.forEach((schema) => {
            const { name, category } = schema;
            categoryDict[name] = category;
        });
        const data: Record<string, unknown> = {};

        Object.keys(values).forEach((key) => {
            switch (categoryDict[key]) {
                case Category.Json: {
                    const value = values[key];
                    if (value && isObjectLike(value)) {
                        data[key] = JSON.stringify(value, null, 2);
                    } else {
                        data[key] = value;
                    }
                    break;
                }
                default:
                    data[key] = values[key];
                    break;
            }
        });
        return data;
    }

    async function handleUpdate(values: Record<string, unknown>) {
        const updateData: Record<string, unknown> = {};
        Object.keys(form.formState.dirtyFields).forEach((key) => {
            if (form.formState.dirtyFields[key]) {
                updateData[key] = values[key];
            }
        });
        if (Object.keys(updateData).length === 0) {
            toast.info(i18nModelEditor("noChange"));
            return;
        }

        await modelUpdate({
            id: modelId,
            model: modelName || "",
            data: formatValue(updateData),
        });
        const data = Object.assign({}, detail, updateData);
        setDetail(data);
        form.reset(formatFormValue(schemaView, data));
        toast.success(i18nModelEditor("updateSuccess"));
    }
    async function handleCreate(values: Record<string, unknown>) {
        await modelCreate({
            model: modelName || "",
            data: formatValue(values),
        });
        toast.success(i18nModelEditor("createSuccess"));
        goBack();
    }

    async function onSubmit(values: Record<string, unknown>) {
        if (processing) {
            return;
        }
        setProcessing(true);
        try {
            if (editType === EditType.Create) {
                await handleCreate(values);
            } else {
                await handleUpdate(values);
            }
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setProcessing(false);
        }
    }

    useAsync(async () => {
        try {
            const id = Number(modelId);
            const model = modelName || "";
            const schema = await fetchSchema(model);
            if (id) {
                const data = await modelDetail({
                    id,
                    model,
                });
                setDetail(data);
                form.reset(formatFormValue(schema, data));
            } else {
                form.reset(formatDefaultValue(schema));
            }
        } catch (err) {
            toast.error(formatError(err));
        } finally {
            setInitialized(true);
        }
    }, [modelName, modelId]);

    if (!initialized) {
        return <Loading />;
    }

    function formatFieldName(name: string): string {
        return name.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
    }

    const labelMap = Object.fromEntries(
        schemaView.schemas.map((s) => [s.name, s.label || formatFieldName(s.name)]),
    );

    const renderFormField = (
        name: string,
        fn: (field: ControllerRenderProps) => React.ReactNode,
    ) => {
        return (
            <FormField
                control={form.control}
                name={name}
                render={({ field }) => (
                    <FormItem>
                        <FormLabel>{labelMap[name] || name}</FormLabel>
                        <FormControl>{fn(field)}</FormControl>
                    </FormItem>
                )}
            />
        );
    };

    const formItems = schemaView.schemas
        .filter((schema) => {
            if (schema.name === "id") {
                return false;
            }
            if (editType === EditType.Create && schema.auto_create) {
                return false;
            }
            if (
                editType === EditType.Edit &&
                ["created", "modified"].includes(schema.name)
            ) {
                return false;
            }
            return true;
        })
        .map((schema) => {
            const { name, category, options } = schema;
            let disabled = editType == EditType.View;
            if (editType === EditType.Edit) {
                disabled = schema.read_only;
            }
            let valueField = renderFormField(name, (field) => {
                let dom = (
                    <Input
                        {...field}
                        value={field.value || ""}
                        disabled={disabled}
                        readOnly={disabled}
                    />
                );
                if (category === Category.Json) {
                    dom = (
                        <Textarea
                            {...field}
                            value={field.value || ""}
                            disabled={disabled}
                            readOnly={disabled}
                        />
                    );
                }
                return dom;
            });
            switch (category) {
                case Category.Status: {
                    valueField = renderFormField(name, (field) => {
                        return (
                            <StatusBadge
                                status={toString(field.value)}
                                i18nModel={i18nModel}
                            />
                        );
                    });
                    break;
                }
                case Category.Result: {
                    valueField = renderFormField(name, (field) => {
                        return (
                            <ResultBadge
                                className="mt-1"
                                result={field.value}
                            />
                        );
                    });
                    break;
                }
                case Category.Placeholder: {
                    valueField = <></>;
                    break;
                }
            }

            let canEdit = true;
            if (editType === EditType.View) {
                canEdit = false;
            }
            if (editType === EditType.Edit && schema.read_only) {
                canEdit = false;
            }
            if (canEdit) {
                switch (category) {
                    case Category.Strings: {
                        if (options) {
                            valueField = renderFormField(name, (field) => {
                                return (
                                    <MultiSelect
                                        {...field}
                                        disabled={disabled}
                                        options={
                                            options?.map((o) => ({
                                                label: o.label,
                                                value: String(o.value),
                                            })) || []
                                        }
                                        selected={
                                            (form.getValues(
                                                name,
                                            ) as string[]) || []
                                        }
                                        onChange={(value) =>
                                            field.onChange(
                                                value.filter(
                                                    (item) => item.length !== 0,
                                                ),
                                            )
                                        }
                                    />
                                );
                            });
                        } else {
                            valueField = renderFormField(name, (field) => {
                                return (
                                    <Textarea
                                        {...field}
                                        value={field.value || ""}
                                        disabled={disabled}
                                        readOnly={disabled}
                                        onChange={(e) => {
                                            const value = e.target.value.trim();
                                            field.onChange(value.split(","));
                                        }}
                                    />
                                );
                            });
                        }
                        break;
                    }
                    case Category.Status: {
                        valueField = renderFormField(name, (field) => {
                            return (
                                <StatusRadioGroup
                                    {...field}
                                    status={toString(field.value)}
                                    i18nModel={i18nModel}
                                    onValueChange={(value) =>
                                        field.onChange(Number(value))
                                    }
                                />
                            );
                        });
                        break;
                    }
                    case Category.Number: {
                        if (options) {
                            valueField = renderFormField(name, (field) => {
                                return (
                                    <Select
                                        {...field}
                                        onValueChange={(value) => {
                                            field.onChange(Number(value));
                                        }}
                                    >
                                        <SelectTrigger className="w-full">
                                            <SelectValue placeholder={name} />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectGroup>
                                                <SelectLabel>
                                                    {name}
                                                </SelectLabel>
                                                {options.map((option) => (
                                                    <SelectItem
                                                        key={String(option.value)}
                                                        value={String(option.value)}
                                                    >
                                                        {option.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectGroup>
                                        </SelectContent>
                                    </Select>
                                );
                            });
                        } else {
                            valueField = renderFormField(name, (field) => {
                                return (
                                    <Input
                                        {...field}
                                        value={
                                            field.value === null ||
                                                field.value === undefined
                                                ? ""
                                                : field.value
                                        }
                                        disabled={disabled}
                                        readOnly={disabled}
                                        type="number"
                                        onChange={(e) => {
                                            const value = e.target.value;
                                            field.onChange(
                                                value === ""
                                                    ? null
                                                    : Number(value),
                                            );
                                        }}
                                    />
                                );
                            });
                        }
                        break;
                    }
                    case Category.Search: {
                        valueField = renderFormField(name, (field) => {
                            return (
                                <SearchSelect
                                    ref={field.ref}
                                    value={toString(field.value)}
                                    disabled={disabled}
                                    searchModel={schema.search_model ?? ""}
                                    onSearch={(model, keyword) =>
                                        modelSearch({
                                            model,
                                            keyword,
                                        })
                                    }
                                    onChange={field.onChange}
                                    onBlur={field.onBlur}
                                />
                            );
                        });
                        break;
                    }
                    case Category.Boolean: {
                        valueField = renderFormField(name, (field) => {
                            return (
                                <Switch
                                    className="mt-2"
                                    {...field}
                                    checked={field.value}
                                    onCheckedChange={(value) => {
                                        field.onChange(value);
                                    }}
                                />
                            );
                        });
                        break;
                    }
                    case Category.Date: {
                        valueField = renderFormField(name, (field) => {
                            return (
                                <DateTimePicker
                                    {...field}
                                    date={field.value}
                                    i18n={i18nComponent}
                                    setDate={(date) => {
                                        if (date) {
                                            field.onChange(
                                                dayjs(date).format(
                                                    "YYYY-MM-DDTHH:mm:ssZ",
                                                ),
                                            );
                                        } else {
                                            field.onChange(null);
                                        }
                                    }}
                                />
                            );
                        });
                        break;
                    }
                    case Category.Json: {
                        valueField = renderFormField(name, (field) => {
                            return (
                                <Textarea
                                    {...field}
                                    value={field.value || ""}
                                    disabled={disabled}
                                    readOnly={disabled}
                                />
                            );
                        });
                        break;
                    }
                    case Category.Code: {
                        valueField = renderFormField(name, (field) => {
                            return (
                                <Textarea
                                    {...field}
                                    value={field.value || ""}
                                    disabled={disabled}
                                    readOnly={disabled}
                                />
                            );
                        });
                        break;
                    }
                    default: {
                        if (options) {
                            valueField = renderFormField(name, (field) => {
                                return (
                                    <Select
                                        {...field}
                                        onValueChange={(value) => {
                                            field.onChange(value);
                                        }}
                                    >
                                        <SelectTrigger className="w-full">
                                            <SelectValue placeholder={name} />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectGroup>
                                                <SelectLabel>
                                                    {name}
                                                </SelectLabel>
                                                {options.map((option) => (
                                                    <SelectItem
                                                        key={String(option.value)}
                                                        value={String(option.value)}
                                                    >
                                                        {option.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectGroup>
                                        </SelectContent>
                                    </Select>
                                );
                            });
                        }
                        break;
                    }
                }
            }
            const span = schema.span || 1;
            return (
                <div
                    className={cn(
                        "gap-2",
                        span === 1 ? "col-span-1" : "",
                        span === 2 ? "col-span-2" : "",
                        span === 3 ? "col-span-3" : "",
                        span === 4 ? "col-span-4" : "",
                    )}
                    key={name}
                >
                    {valueField}
                </div>
            );
        });
    return (
        <div>
            <h1>
                {modelName?.toUpperCase()}{" "}
                {modelId !== 0 && <Badge variant="secondary">{modelId}</Badge>}
            </h1>
            <Form {...form}>
                <form className="mt-4" onSubmit={form.handleSubmit(onSubmit)}>
                    <div className="grid grid-cols-2 gap-4">{formItems}</div>
                    <div className="flex gap-4 w-full mt-4">
                        {editType !== EditType.View && (
                            <Button
                                className=" mb-4 flex-1"
                                type="submit"
                                disabled={!form.formState.isDirty}
                            >
                                {processing && (
                                    <Loader2Icon className="size-4 animate-spin mr-2" />
                                )}
                                {editType === EditType.Create
                                    ? i18nModelEditor("create")
                                    : i18nModelEditor("update")}
                            </Button>
                        )}
                        <Button
                            className="mb-4 flex-1"
                            variant="secondary"
                            onClick={(e) => {
                                e.preventDefault();
                                goBack();
                            }}
                        >
                            {i18nModelEditor("back")}
                        </Button>
                    </div>
                </form>
            </Form>
        </div>
    );
}
