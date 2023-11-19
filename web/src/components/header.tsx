import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { LogOut, Check, User2 } from "lucide-react";
import { Link } from "react-router-dom";
import { HOME, LOGIN } from "@/data/route";
import useUserStore from "@/state/user";
import { useTheme } from "@/components/theme-provider";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

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
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="link">
              <User2 className="mr-2 h-4 w-4" />
              {account}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="w-56">
            <DropdownMenuLabel>My Account</DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem
                className="cursor-pointer"
                onClick={() => {
                  logout();
                }}
              >
                <LogOut className="mr-2 h-4 w-4" />
                <span>Log out</span>
              </DropdownMenuItem>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem
                className="cursor-pointer"
                onClick={() => {
                  setTheme("system");
                }}
              >
                {theme == "system" && <Check className="mr-2 h-4 w-4" />}
                <span>System Theme</span>
              </DropdownMenuItem>
              <DropdownMenuItem
                className="cursor-pointer"
                onClick={() => {
                  setTheme("dark");
                }}
              >
                {theme == "dark" && <Check className="mr-2 h-4 w-4" />}
                <span>Dark Theme</span>
              </DropdownMenuItem>
              <DropdownMenuItem
                className="cursor-pointer"
                onClick={() => {
                  setTheme("light");
                }}
              >
                {theme == "light" && <Check className="mr-2 h-4 w-4" />}
                <span>Light Theme</span>
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
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
        </div>
      </div>
    </header>
  );
}
