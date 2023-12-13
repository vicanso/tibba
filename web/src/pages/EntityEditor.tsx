import { MainHeader } from "@/components/header";
import { useParams } from "react-router-dom";
import EntityForm from "@/components/entity-form";
import { useAsync } from "react-async-hook";
import useEntityStore, { EntityDescription } from "@/state/entity";
import { useState } from "react";
import { toast } from "@/components/ui/use-toast";
import { formatError } from "@/helpers/util";

export default function EntityEditor() {
  const { entity, id } = useParams();
  const [fetchDescription] = useEntityStore((state) => [
    state.fetchDescription,
  ]);
  const [initialized, setInitialized] = useState(false);
  const [description, setDescription] = useState<EntityDescription>(
    {} as EntityDescription,
  );

  useAsync(async () => {
    setInitialized(false);
    try {
      const result = await fetchDescription(entity || "");
      setDescription(result);
    } catch (err) {
      toast({
        title: "获取描述信息失败",
        description: formatError(err),
      });
      console.error(err);
    } finally {
      setInitialized(true);
    }
  }, [entity]);
  if (!initialized) {
    return <div>...</div>;
  }
  return (
    <div>
      <MainHeader />
      <EntityForm
        entity={entity || ""}
        id={id || ""}
        description={description}
      />
    </div>
  );
}
