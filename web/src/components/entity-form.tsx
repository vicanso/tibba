import { useAsync } from "react-async-hook";
import useEntityStore, {
  EntityDescription,
  getEntityStatusOptions,
  EntityItemCategory,
} from "@/state/entity";
import { Card, CardHeader } from "@/components/ui/card";
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
} from "@/components/ui/form";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import { isNil } from "lodash-es";
import { useState } from "react";
import { Input } from "@/components/ui/input";
import { useForm } from "react-hook-form";
import * as z from "zod";
import { zodResolver } from "@hookform/resolvers/zod";

export default function EntityForm({
  entity,
  id,
}: {
  entity: string;
  id: string;
}) {
  const [fetchDescription, getEntity] = useEntityStore((state) => [
    state.fetchDescription,
    state.getEntity,
  ]);
  const [description, setDescription] = useState<EntityDescription>(
    {} as EntityDescription,
  );
  const [entityData, setEntityData] = useState<Record<string, unknown>>({});
  const schema: Record<string, z.ZodTypeAny> = {};
  useAsync(async () => {
    const data = await getEntity(entity, id);
    setEntityData(data);

    const result = await fetchDescription(entity);
    result.items.forEach((item) => {
      schema[item.name] = z.string().min(2).max(50);
    });

    setDescription(result);
  }, [entity, id]);

  const form = useForm({
    resolver: zodResolver(z.object(schema)),
  });

  async function onSubmit(values: Record<string, unknown>) {
    console.dir(values);
  }

  const formItems = description.items
    ?.filter((item) => item.name !== "id")
    .map((item) => {
      const originValue = entityData[item.name];
      let value = String(originValue);
      if (isNil(originValue)) {
        value = "";
      }
      return (
        <FormField
          key={item.name}
          name={item.name}
          control={form.control}
          render={({ field }) => {
            let element: JSX.Element;
            switch (item.category) {
              case EntityItemCategory.STATUS: {
                const options = getEntityStatusOptions().map((item) => {
                  return (
                    <SelectItem key={item.value} value={item.value}>
                      {item.label}
                    </SelectItem>
                  );
                });
                element = (
                  <Select defaultValue={value}>
                    <SelectTrigger>
                      <SelectValue placeholder="请选择" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>{options}</SelectGroup>
                    </SelectContent>
                  </Select>
                );
                break;
              }
              default: {
                element = (
                  <Input
                    defaultValue={value}
                    {...field}
                    disabled={item.readonly}
                  />
                );
                break;
              }
            }
            return (
              <FormItem>
                <FormLabel>{item.label}</FormLabel>
                <FormControl>{element}</FormControl>
              </FormItem>
            );
          }}
        />
      );
    });

  return (
    <div className="w-full">
      <Card className="m-5">
        <CardHeader>数据更新-记录Id：{id}</CardHeader>
        <Form {...form}>
          <div className="ml-5 mr-5 mb-5">
            <form onSubmit={form.handleSubmit(onSubmit)}>
              <div className="grid grid-cols-3 gap-4">{formItems}</div>
            </form>
          </div>
        </Form>
      </Card>
    </div>
  );
}
