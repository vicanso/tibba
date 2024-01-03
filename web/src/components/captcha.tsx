import { cn } from "@/lib/utils";
import { Input } from "@/components/ui/input";
import { useAsync } from "react-async-hook";

import { COMMON_CAPTCHA } from "@/url";
import request from "@/helpers/request";
import { formatError } from "@/helpers/util";
import { useState } from "react";
import { toast } from "@/components/ui/use-toast";

interface CaptchaProps {
  className?: string;
  level?: number;
  onChange: (value: string) => void;
}

interface CaptchaData {
  ts: number;
  hash: string;
  data: string;
}

export function Captcha({ className, level = 0, onChange }: CaptchaProps) {
  const [captchaData, setCaptchaData] = useState({} as CaptchaData);
  const [refreshCount, setRefreshCount] = useState(0);

  useAsync(async () => {
    try {
      const { data } = await request.get<{
        ts: number;
        hash: string;
        data: string;
      }>(COMMON_CAPTCHA, {
        params: {
          level: level,
        },
      });
      setCaptchaData(data);
    } catch (err) {
      toast({
        title: "获取图形验证码失败",
        description: formatError(err),
      });
    }
  }, [refreshCount]);

  return (
    <div className={cn("", className)}>
      {captchaData.data && (
        <img
          className="cursor-pointer float-right mt-[1px]"
          src={`data:image/png;base64,${captchaData.data}`}
          onClick={() => {
            setRefreshCount(refreshCount + 1);
          }}
        />
      )}
      <div className="mr-[135px]">
        <Input
          placeholder="请输入验证码"
          onChange={(e) => {
            const { value } = e.target;
            // 输出4个字符以上才触发
            if (value.length >= 4) {
              onChange(`${captchaData.ts}:${captchaData.hash}:${value}`);
            }
          }}
        />
      </div>
    </div>
  );
}
