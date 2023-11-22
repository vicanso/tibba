import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { LogIn, LogOut, Check, User2, Sun, Moon, SunMoon } from "lucide-react";
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
  const iconClassName = "mr-2 h-4 w-4";
  if (!loading) {
    if (account) {
      tips = (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="link">
              <User2 className="h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="w-56">
            <DropdownMenuLabel>{account}</DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuItem
                className="cursor-pointer"
                onClick={() => {
                  logout();
                }}
              >
                <LogOut className={iconClassName} />
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
                {theme == "system" && <Check className={iconClassName} />}
                {theme != "system" && <SunMoon className={iconClassName} />}
                <span>System Theme</span>
              </DropdownMenuItem>
              <DropdownMenuItem
                className="cursor-pointer"
                onClick={() => {
                  setTheme("dark");
                }}
              >
                {theme == "dark" && <Check className={iconClassName} />}
                {theme != "dark" && <Moon className={iconClassName} />}
                <span>Dark Theme</span>
              </DropdownMenuItem>
              <DropdownMenuItem
                className="cursor-pointer"
                onClick={() => {
                  setTheme("light");
                }}
              >
                {theme == "light" && <Check className={iconClassName} />}
                {theme != "light" && <Sun className={iconClassName} />}
                <span>Light Theme</span>
              </DropdownMenuItem>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      );
    } else {
      tips = (
        <Button variant="link">
          <Link to={LOGIN}>
            <LogIn className="mr-2 h-4 w-4 inline" />
            Login
          </Link>
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
        <div className="flex flex-1 items-center justify-between space-x-2 md:justify-end mr-5">
          {tips}
        </div>
      </div>
    </header>
  );
}
