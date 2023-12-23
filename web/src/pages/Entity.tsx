import { MainSidebar } from "@/components/sidebar-nav";
import { MainHeader } from "@/components/header";
import { useParams } from "react-router-dom";

import EntityTable from "@/components/entity-table";

export default function Entity() {
  const { entity } = useParams();
  return (
    <div>
      <MainHeader />
      <div className="flex">
        <MainSidebar className="h-screen flex-none w-[230px]" />
        <div className="grow lg:border-l">
          <EntityTable entity={entity || ""} />
        </div>
      </div>
    </div>
  );
}
