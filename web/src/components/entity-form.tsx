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
import { cn } from "@/lib/utils";
import { Calendar as CalendarIcon } from "lucide-react";
import { Calendar } from "@/components/ui/calendar";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Textarea } from "@/components/ui/textarea";

import { Button } from "@/components/ui/button";
import { isNil, includes, isEqual } from "lodash-es";
import { useState } from "react";
import { Input } from "@/components/ui/input";
import { useForm } from "react-hook-form";
import * as z from "zod";
import { zodResolver } from "@hookform/resolvers/zod";
import { toast } from "@/components/ui/use-toast";
import { formatDate, formatError } from "@/helpers/util";
import { Label } from "@/components/ui/label";
import dayjs from "dayjs";
import { Loading } from "@/components/loading";

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
  const [getEntity, updateEntity] = useEntityStore((state) => [
    state.getEntity,
    state.updateEntity,
  ]);
  const [tips, setTips] = useState("");
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
    const keys = Object.keys(updateData);
    if (keys.length === 0) {
      toast({
        title: "数据未修改",
        description: "当前的数据未有修改，请修改后再提交",
      });
      return;
    }
    setTips("");
    setProcessing(true);
    try {
      await updateEntity(entity, id, updateData);
      keys.forEach((key) => {
        entityData[key] = updateData[key];
      });
      setTips(`已成功更新数据，字段为：${keys.join(",")}。`);
    } finally {
      setProcessing(false);
    }
  }

  if (!entityData.id) {
    return <Loading />;
  }

  const formItems = description.items
    ?.filter((item) => !includes(ignoreFields, item.name))
    .map((item) => {
      let fieldClass = "";
      if (item.span) {
        fieldClass = `col-span-${item.span}`;
      }
      return (
        <div key={item.name} className={fieldClass}>
          <FormField
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
                case EntityItemCategory.DATETIME: {
                  const date = field.value;
                  let time = "";
                  if (date) {
                    const arr = formatDate(date).split(" ");
                    if (arr.length === 2) {
                      time = arr[1];
                    }
                  }
                  const footer = (
                    <>
                      <div className="px-4 pt-0 pb-4">
                        <Label>时间</Label>
                        <Input
                          className="mt-[5px]"
                          type="time"
                          onChange={(e) => {
                            const { value } = e.target;
                            const hours = Number.parseInt(
                              value.split(":")[0] || "00",
                              10,
                            );
                            const minutes = Number.parseInt(
                              value.split(":")[1] || "00",
                              10,
                            );
                            const datetime = dayjs(date)
                              .hour(hours)
                              .minute(minutes)
                              .toISOString();
                            form.setValue(item.name, datetime);
                          }}
                          defaultValue={time}
                        />
                      </div>
                      {!date && <p>Please pick a day.</p>}
                    </>
                  );
                  element = (
                    <Popover key={item.name}>
                      <PopoverTrigger asChild>
                        <div>
                          <Button
                            variant={"outline"}
                            type="button"
                            className={cn(
                              "justify-start text-left font-normal w-full",
                              !date && "text-muted-foreground",
                            )}
                          >
                            <CalendarIcon className="mr-2 h-4 w-4" />
                            {date ? formatDate(date) : <span>请选择日期</span>}
                          </Button>
                        </div>
                      </PopoverTrigger>
                      <PopoverContent className="w-auto p-0">
                        <Calendar
                          mode="single"
                          selected={new Date(date)}
                          onSelect={(value) => {
                            if (value) {
                              form.setValue(item.name, value.toISOString());
                            } else {
                              form.setValue(item.name, "");
                            }
                          }}
                          initialFocus
                        />
                        {footer}
                      </PopoverContent>
                    </Popover>
                  );
                  break;
                }
                case EntityItemCategory.EDITOR: {
                  element = (
                    <Textarea {...field} disabled={item.readonly} rows={8} />
                  );
                  break;
                }
                default: {
                  element = <Input {...field} disabled={item.readonly} />;
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
        </div>
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
              <Label className="ml-5">{tips}</Label>
            </CardFooter>
          </Card>
        </form>
      </Form>
    </div>
  );
}
