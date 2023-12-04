import { MainHeader } from "@/components/header";
import { useParams } from "react-router-dom";

export default function EntityForm() {
  const { entity, id } = useParams();
  console.dir(entity);
  console.dir(id);
  return (
    <div>
      <MainHeader />
    </div>
  );
}
