import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Link } from "react-router-dom";

import router from "@/router";
import { navItemList } from "@/router.tsx";

interface MainSidebarProps extends React.HTMLAttributes<HTMLDivElement> {}

export function MainSidebar({ className }: MainSidebarProps) {
  const { pathname } = router.state.location;
  const arr = navItemList.map((item) => {
    const subItems = item.items.map((subItem) => {
      const variant = pathname == subItem.url ? "secondary" : "ghost";
      return (
        <Button
          variant={variant}
          className="w-full justify-start"
          key={`${item.name}-${subItem.name}`}
        >
          <Link className="block text-left w-full leading-10" to={subItem.url}>
            {subItem.name}
          </Link>
        </Button>
      );
    });
    return (
      <div className="px-3 py-2" key={item.name}>
        <h2 className="px-4 text-lg">
          {item.name}
          <div className="space-y-1">{subItems}</div>
        </h2>
      </div>
    );
  });
  return (
    <div className={cn("pb-12", className)}>
      <div className="space-y-1 py-4">{arr}</div>
    </div>
  );
}
