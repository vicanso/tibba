import { MainHeader } from "@/components/header";
import { useParams } from "react-router-dom";
import EntityForm from "@/components/entity-form";

export default function EntityEditor() {
  const { entity, id } = useParams();
  console.dir(entity);
  console.dir(id);
  return (
    <div>
      <MainHeader />
      <EntityForm entity={entity || ""} id={id || ""} />
    </div>
  );
}
