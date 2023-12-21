import { cn } from "@/lib/utils";

import loadingDark from "@/asset/loading-dark.gif";
import loadingWhite from "@/asset/loading-white.gif";
import { useTheme } from "@/components/theme-provider";

interface LoadingProps extends React.HTMLAttributes<HTMLDivElement> {
  tips?: string;
}

export function Loading({ className, tips }: LoadingProps) {
  const { theme } = useTheme();
  const imgSrc = theme === "dark" ? loadingDark : loadingWhite;
  return (
    <div className={cn("text-center m-4", className)}>
      <p className="text-sm leading-[32px]">
        <img className="inline w-[32px]" src={imgSrc} />
        {tips || "数据加载中，请稍候..."}
      </p>
    </div>
  );
}
