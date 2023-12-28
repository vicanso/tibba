import request from "@/helpers/request";
import {
  INNER_ENTITIES,
  INNER_ENTITY_DESCRIPTIONS,
  INNER_ENTITIES_ID,
} from "@/url";
import { create } from "zustand";

interface EntityOption {
  label: string;
  str_value: string;
  num_value: number;
}

interface EntityItem {
  name: string;
  label: string;
  category: string;
  readonly: boolean;
  width: number;
  options: EntityOption[];
  span: number;
}
export interface EntityDescription {
  items: EntityItem[];
  support_orders: string[];
  modify_roles: string[];
}

export enum EntityItemCategory {
  TEXT = "text",
  NUMBER = "number",
  DATETIME = "datetime",
  EDITOR = "editor",
  TEXTS = "texts",
  STATUS = "status",
  JSON = "json",
}

enum EntityStatus {
  DISABLED = 0,
  ENABLED = 1,
}

export function formatEntityStatus(value: unknown) {
  if (Number(value) === EntityStatus.ENABLED) {
    return "启用";
  }
  return "禁用";
}

export function getEntityStatusOptions() {
  return [EntityStatus.ENABLED, EntityStatus.DISABLED].map((value) => {
    return {
      value: String(value),
      label: formatEntityStatus(value),
    };
  });
}

interface EntityState {
  fetchDescription: (entity: string) => Promise<EntityDescription>;
  listEntities: (params: {
    entity: string;
    page: number;
    page_size: number;
    keyword: string;
    orders: string;
    counted: boolean;
  }) => Promise<{
    page_count: number;
    items: Record<string, unknown>[];
  }>;
  getEntity: (entity: string, id: string) => Promise<Record<string, unknown>>;
  updateEntity: (
    entity: string,
    id: string,
    data: Record<string, unknown>,
  ) => Promise<void>;
}

const entityStore = create<EntityState>()(() => ({
  fetchDescription: async (entity: string) => {
    const { data } = await request.get<EntityDescription>(
      INNER_ENTITY_DESCRIPTIONS.replace(":entity", entity),
    );
    return data;
  },
  listEntities: async ({
    entity,
    page,
    page_size,
    keyword,
    orders,
    counted,
  }: {
    entity: string;
    page: number;
    page_size: number;
    keyword: string;
    orders: string;
    counted: boolean;
  }) => {
    const { data } = await request.get<{
      page_count: number;
      items: Record<string, unknown>[];
    }>(INNER_ENTITIES.replace(":entity", entity), {
      params: {
        page_size,
        page,
        orders,
        keyword,
        counted,
      },
    });
    return data;
  },
  getEntity: async (entity: string, id: string) => {
    const url = INNER_ENTITIES_ID.replace(":entity", entity).replace(":id", id);
    const { data } = await request.get<Record<string, unknown>>(url);
    return data;
  },
  updateEntity: async (
    entity: string,
    id: string,
    data: Record<string, unknown>,
  ) => {
    const url = INNER_ENTITIES_ID.replace(":entity", entity).replace(":id", id);
    await request.patch(url, data);
  },
}));

export default entityStore;
