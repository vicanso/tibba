import { MainSidebar } from "@/components/sidebar-nav";
import { MainHeader } from "@/components/header";
import { useParams } from "react-router-dom";

import EntityTable from "@/components/entity-table";

export default function Entity() {
  const { entity } = useParams();
  console.dir(entity);
  return (
    <div>
      <MainHeader />
      <div className="grid lg:grid-cols-5">
        <MainSidebar className="h-screen" />
        <div className="col-span-3 lg:col-span-4 lg:border-l">
          <EntityTable entity={entity || ""} />
        </div>
      </div>
    </div>
  );
}
