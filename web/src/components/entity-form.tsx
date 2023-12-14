import { useAsync } from "react-async-hook";
import useEntityStore, {
  EntityDescription,
  getEntityStatusOptions,
  EntityItemCategory,
} from "@/state/entity";
import { Card, CardHeader, CardFooter } from "@/components/ui/card";
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
import { Button } from "@/components/ui/button";
import { isNil, includes, isEqual } from "lodash-es";
import { useState } from "react";
import { Input } from "@/components/ui/input";
import { useForm } from "react-hook-form";
import * as z from "zod";
import { zodResolver } from "@hookform/resolvers/zod";
import { toast } from "@/components/ui/use-toast";
import { formatError } from "@/helpers/util";

const ignoreFields = ["id", "created_at", "updated_at"];

export default function EntityForm({
  entity,
  id,
  description,
}: {
  entity: string;
  id: string;
  description: EntityDescription;
}) {
  const [getEntity] = useEntityStore((state) => [state.getEntity]);
  const [entityData, setEntityData] = useState<Record<string, unknown>>({});
  const [processing, setProcessing] = useState<boolean>(false);

  const schema: Record<string, z.ZodTypeAny> = {};

  description.items.forEach((item) => {
    switch (item.category) {
      case EntityItemCategory.NUMBER: {
        schema[item.name] = z.number();
        break;
      }
      case EntityItemCategory.STATUS: {
        schema[item.name] = z.number();
        break;
      }
      case EntityItemCategory.TEXTS: {
        schema[item.name] = z.array(z.string());
        break;
      }
      default: {
        schema[item.name] = z.string().min(0).max(50);
        break;
      }
    }
  });
  const form = useForm({
    resolver: zodResolver(z.object(schema)),
  });

  useAsync(async () => {
    try {
      const data = await getEntity(entity, id);
      Object.keys(data).forEach((key) => {
        if (isNil(data[key])) {
          data[key] = "";
        }
        form.setValue(key, data[key]);
      });
      setEntityData(data);
    } catch (err) {
      toast({
        title: "获取数据失败",
        description: formatError(err),
      });
    }
  }, [entity, id]);

  async function onSubmit(values: Record<string, unknown>) {
    if (processing) {
      return;
    }
    const updateData: Record<string, unknown> = {};
    Object.keys(values).forEach((key) => {
      if (!isEqual(values[key], entityData[key])) {
        updateData[key] = values[key];
      }
    });
    if (Object.keys(updateData).length === 0) {
      toast({
        title: "数据未修改",
        description: "当前的数据未有修改，请修改后再提交",
      });
      return;
    }
    setProcessing(true);
    try {
      console.dir(updateData);
    } finally {
      setProcessing(false);
    }
  }

  if (!entityData.id) {
    return <div>...</div>;
  }

  const formItems = description.items
    ?.filter((item) => !includes(ignoreFields, item.name))
    .map((item) => {
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

                // TODO 是否可以支持number类型
                element = (
                  <Select
                    defaultValue={String(field.value)}
                    onValueChange={(value) => {
                      form.setValue(item.name, Number(value));
                    }}
                  >
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
              case EntityItemCategory.TEXTS: {
                const options = item.options.map((item) => {
                  return (
                    <SelectItem key={item.str_value} value={item.str_value}>
                      {item.label}
                    </SelectItem>
                  );
                });
                // TODO 是否支持multiple
                element = (
                  <Select
                    defaultValue={field.value[0]}
                    onValueChange={(value) => {
                      form.setValue(item.name, [value]);
                    }}
                  >
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
                    // defaultValue={value}
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
      <Form {...form}>
        <form onSubmit={form.handleSubmit(onSubmit)}>
          <Card className="m-5">
            <CardHeader>数据更新-记录Id：{id}</CardHeader>
            <div className="ml-5 mr-5 mb-5">
              <div className="grid grid-cols-3 gap-4">{formItems}</div>
            </div>
            <CardFooter>
              <Button type="submit" className="w-[150px]">
                {processing ? "更新中..." : "更新"}
              </Button>
            </CardFooter>
          </Card>
        </form>
      </Form>
    </div>
  );
}
