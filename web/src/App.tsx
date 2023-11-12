import { Button } from "@/components/ui/button";
import { Link } from "react-router-dom";
import { cn } from "@/lib/utils";
import { Sidebar } from "@/components/sidebar-nav";
import { navItemList } from "@/router.tsx";

export default function Home() {
  return (
    <div>
      <div className="grid lg:grid-cols-5">
        <Sidebar navItemList={navItemList} />
      </div>
      <div>
        <Button>Click me</Button>
        <Link to={"/test"}>abc</Link>
      </div>
    </div>
  );
}
