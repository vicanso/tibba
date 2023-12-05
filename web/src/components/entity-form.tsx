import { useAsync } from "react-async-hook";
import request from "@/helpers/request";
import { INNER_ENTITIES_DETAIL } from "@/url";

async function getEntity(entity: string, id: string) {
  const url = INNER_ENTITIES_DETAIL.replace(":entity", entity).replace(
    ":id",
    id,
  );
  const { data } = await request.get<Map<string, unknown>>(url);
  return data;
}

export default function EntityForm({
  entity,
  id,
}: {
  entity: string;
  id: string;
}) {
  useAsync(async () => {
    const data = await getEntity(entity, id);
    console.dir(data);
  }, [entity, id]);
  return (
    <div className="w-full">
      {entity}
      {id}
    </div>
  );
}
