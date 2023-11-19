import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

import { Link } from "react-router-dom";
import { HOME, LOGIN } from "@/data/route";
import useUserStore from "@/state/user";
import { useTheme, Theme } from "@/components/theme-provider";

interface MainHeaderProps extends React.HTMLAttributes<HTMLDivElement> {}

export function MainHeader({ className }: MainHeaderProps) {
  const [account, loading] = useUserStore((state) => [
    state.account,
    state.loading,
  ]);
  const { setTheme, theme } = useTheme();

  const logout = useUserStore((state) => state.logout);
  let tips = <span className="mr-2">Loading...</span>;
  if (!loading) {
    if (account) {
      tips = (
        <>
          {account}
          <Button
            variant="link"
            onClick={() => {
              logout();
            }}
          >
            Logout
          </Button>
        </>
      );
    } else {
      tips = (
        <Button variant="link">
          <Link to={LOGIN}>Login</Link>
        </Button>
      );
    }
  }

  return (
    <header
      className={cn(
        "sticky top-0 z-50 w-full border-b bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/60",
        className,
      )}
    >
      <div className="ml-10 flex h-14 items-center">
        <Link to={HOME} className="font-bold">
          Tibba
        </Link>
        <div className="flex flex-1 items-center justify-between space-x-2 md:justify-end mr-20">
          {tips}
          <Select
            defaultValue={theme}
            onValueChange={(value) => {
              setTheme(value as Theme);
            }}
          >
            <SelectTrigger className="w-[120px]">
              <SelectValue placeholder="Theme" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="light">Light</SelectItem>
              <SelectItem value="dark">Dark</SelectItem>
              <SelectItem value="system">System</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>
    </header>
  );
}
