import { MainSidebar } from "@/components/sidebar-nav";
import { MainHeader } from "@/components/header";

export default function Home() {
  return (
    <div>
      <MainHeader />
      <div className="flex">
        <MainSidebar className="h-screen flex-none w-[230px]" />
        <div className="grow lg:border-l"></div>
      </div>
    </div>
  );
}
