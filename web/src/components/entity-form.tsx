import { useAsync } from "react-async-hook";
import useEntityStore, { EntityDescription } from "@/state/entity";
import { Card } from "@/components/ui/card";
import {
  Form,
  FormControl,
  FormDescription,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { useState } from "react";
import { Input } from "./ui/input";

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
  useAsync(async () => {
    const result = await fetchDescription(entity);
    setDescription(result);
    console.dir(description);
    const data = await getEntity(entity, id);
    console.dir(data);
  }, [entity, id]);

  const formItems = description.items?.map((item) => {
    console.dir(item);
    return (
      <FormField
        key={item.name}
        name={item.name}
        render={({ field }) => (
          <FormItem>
            <FormLabel>{item.label}</FormLabel>
          </FormItem>
        )}
      />
    );
  });
  console.dir(formItems);

  return (
    <div className="w-full">
      <Card></Card>
      {entity}
      {id}
    </div>
  );
}
