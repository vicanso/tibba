import { MainSidebar } from "@/components/sidebar-nav";
import { MainHeader } from "@/components/header";

import DataTable from "@/components/data-table";

export default function Home() {
  return (
    <div>
      <MainHeader />
      <div className="grid lg:grid-cols-5">
        <MainSidebar className="h-screen" />
        <div className="col-span-3 lg:col-span-4 lg:border-l">
          <DataTable entity="settings" />
        </div>
      </div>
    </div>
  );
}
