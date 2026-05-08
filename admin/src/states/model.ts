import { create } from "zustand";
import {
    MODEL_SCHEMA,
    MODEL_LIST,
    MODEL_DELETE,
    MODEL_DETAIL,
    MODEL_UPDATE,
    MODEL_CREATE,
    MODEL_SEARCH,
} from "@/constants/url";
import request from "@/helpers/request";
import { isNil, toString } from "lodash-es";
import dayjs from "dayjs";

interface Option {
    label: string;
    value: string | number;
}

export enum ConditionCategory {
    Input = "input",
    Date = "date",
    Select = "select",
}

interface Condition {
    name: string;
    label?: string;
    category: ConditionCategory;
    options: Option[];
    defaultValue?: unknown;
}

export enum Category {
    String = "string",
    Number = "number",
    Bytes = "bytes",
    Boolean = "boolean",
    Status = "status",
    Result = "result",
    Strings = "strings",
    ByteSize = "byte_size",
    Date = "date",
    Json = "json",
    Code = "code",
    PopoverCard = "popover_card",
    Placeholder = "placeholder",
    Search = "search",
}

export interface Schema {
    name: string;
    label?: string;
    category: Category;
    read_only: boolean;
    auto_create: boolean;
    required: boolean;
    identity: boolean;
    fixed: boolean;
    options: Option[] | null;
    search_model?: string;
    hidden: boolean;
    sortable: boolean;
    filterable: boolean;
    popover: boolean;
    span?: number;
    default_value: unknown;
    hidden_values: string[];
    max_width?: number;
    combinations?: string[];
}

export interface SchemaView {
    schemas: Schema[];
    conditions: Condition[];
    sort_fields: string[];
    allow_create: {
        disabled: boolean;
        groups: string[];
        roles: string[];
    };
    allow_edit: {
        owner: boolean;
        disabled: boolean;
        groups: string[];
        roles: string[];
    };
}

interface ModelState {
    model: string;
    schemaView: SchemaView;
    initialized: boolean;
    items: Record<string, unknown>[];
    count: number;
    fetchSchema: (name: string) => Promise<SchemaView>;
    list: (params: {
        page: number;
        limit: number;
        keyword?: string;
        order_by?: string;
        filters?: Record<string, string>;
    }) => Promise<void>;
    reset: () => void;
    remove: (params: { id: number; model: string }) => Promise<void>;
    detail: (params: {
        id: number;
        model: string;
    }) => Promise<Record<string, unknown>>;
    update: (params: {
        id: number;
        model: string;
        data: Record<string, unknown>;
    }) => Promise<void>;
    create: (params: {
        model: string;
        data: Record<string, unknown>;
    }) => Promise<{ id: number }>;
    search: (params: {
        model: string;
        keyword: string;
    }) => Promise<{ options: Option[] }>;
}

const useModelState = create<ModelState>((set, get) => ({
    model: "",
    schemaView: {} as SchemaView,
    items: [],
    count: -1,
    initialized: false,
    reset() {
        set({
            items: [],
            count: -1,
        });
    },
    fetchSchema: async (name: string) => {
        set({
            initialized: false,
        });
        const { data } = await request.get<SchemaView>(MODEL_SCHEMA, {
            params: {
                name,
            },
        });
        data.sort_fields = data.schemas
            .filter((schema) => schema.sortable)
            .map((schema) => schema.name);
        data.conditions = data.schemas
            .filter((schema) => schema.filterable)
            .map((schema) => {
                const condition: Condition = {
                    name: schema.name,
                    label: schema.label,
                    category: ConditionCategory.Input,
                    options: schema.options || [],
                };
                if (schema.category === Category.Date) {
                    condition.category = ConditionCategory.Date;
                    condition.defaultValue = [
                        dayjs().subtract(1, "day").startOf("day").toISOString(),
                        dayjs().endOf("day").toISOString(),
                    ];
                } else if (schema.options) {
                    condition.category = ConditionCategory.Select;
                }
                // TODO: 根据schema.category 确定filter options
                return condition;
            });
        set({
            model: name,
            schemaView: data,
            initialized: true,
        });
        return data;
    },
    list: async (params: {
        page: number;
        limit: number;
        keyword?: string;
        filters?: Record<string, string>;
        order_by?: string;
    }) => {
        const { model, count } = get();
        const shouldCount = count < 0;
        let filters = "";
        if (Object.keys(params.filters || {}).length > 0) {
            filters = JSON.stringify(params.filters);
        }
        const { data } = await request.get<{
            count: number;
            items: Record<string, unknown>[];
        }>(MODEL_LIST, {
            params: {
                model,
                count: shouldCount,
                page: params.page,
                limit: params.limit,
                keyword: params.keyword,
                order_by: params.order_by,
                filters,
            },
        });
        // 如果在拉取数据时，model 已经改变，则不更新数据
        if (model != get().model) {
            return;
        }
        const combinationSchemas: Schema[] = [];
        const schemas = get().schemaView.schemas;
        schemas.forEach((schema) => {
            if (schema.combinations) {
                combinationSchemas.push(schema);
            }
        });
        combinationSchemas.forEach((schema) => {
            if (!schema.combinations) {
                return;
            }
            const fields: Schema[] = [];
            schema.combinations.forEach((combination) => {
                const found = schemas.find((item) => item.name === combination);
                if (!found) {
                    return;
                }
                fields.push(found);
            });
            if (fields.length === 0) {
                return;
            }
            data.items.forEach((item) => {
                const values: string[] = [];
                fields.forEach((field) => {
                    const value = item[field.name];
                    const valueString = toString(value);
                    if (
                        isNil(value) ||
                        field.hidden_values.includes(valueString)
                    ) {
                        return;
                    }
                    const name = field.label || field.name;
                    values.push(`${name}: ${valueString}`);
                });
                if (values.length === 0) {
                    return;
                }
                item[schema.name] = values;
            });
        });
        if (shouldCount) {
            set({
                items: data.items,
                count: data.count,
            });
        } else {
            set({
                items: data.items,
            });
        }
    },
    remove: async (params: { id: number; model: string }) => {
        await request.delete(MODEL_DELETE, {
            params: {
                id: params.id,
                model: params.model,
            },
        });
        const { items } = get();
        set({
            items: items.filter((item) => item.id !== params.id),
        });
    },
    detail: async (params: { id: number; model: string }) => {
        const { data } = await request.get<Record<string, unknown>>(
            MODEL_DETAIL,
            {
                params: {
                    id: params.id,
                    model: params.model,
                },
            },
        );
        const { schemaView } = get();
        Object.keys(data).forEach((key) => {
            if (!isNil(data[key])) {
                return;
            }
            const schema = schemaView.schemas.find(
                (schema) => schema.name === key,
            );
            if (!schema) {
                return;
            }
            switch (schema.category) {
                case Category.Strings:
                    data[key] = [];
                    break;
                default:
                    data[key] = "";
                    break;
            }
        });
        return data;
    },
    update: async (params: {
        id: number;
        model: string;
        data: Record<string, unknown>;
    }) => {
        await request.patch(MODEL_UPDATE, params);
    },
    create: async (params: {
        model: string;
        data: Record<string, unknown>;
    }) => {
        const { data } = await request.post<{ id: number }>(
            MODEL_CREATE,
            params,
        );
        return data;
    },
    search: async (params: {
        model: string;
        keyword: string;
    }) => {
        const { data } = await request.get<{ options: Option[] }>(
            MODEL_SEARCH,
            {
                params: {
                    model: params.model,
                    keyword: params.keyword,
                },
            },
        );
        return data;
    },
}));

export default useModelState;
