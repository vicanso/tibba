import { MainHeader } from "@/components/header";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage,
} from "@/components/ui/form";
import { Button } from "@/components/ui/button";
import { zodResolver } from "@hookform/resolvers/zod";
import { useForm } from "react-hook-form";
import * as z from "zod";
import { useState } from "react";
import useUserStore from "@/state/user";
import { goBack } from "@/router";

const loginSchema = z.object({
  account: z.string().min(2).max(50),
  password: z.string().min(6).max(50),
});

export default function Login() {
  const [processing, setProcessing] = useState<boolean>(false);
  const [login, fetch] = useUserStore((state) => [state.login, state.fetch]);

  const form = useForm<z.infer<typeof loginSchema>>({
    resolver: zodResolver(loginSchema),
    defaultValues: {
      account: "",
      password: "",
    },
  });
  async function onSubmit(values: z.infer<typeof loginSchema>) {
    if (processing) {
      return;
    }
    setProcessing(true);
    try {
      await login(values.account, values.password);
      const isLogin = await fetch();
      if (isLogin) {
        goBack();
      }
    } catch (err) {
      // TODO 出错处理
      console.error(err);
    } finally {
      setProcessing(false);
    }
  }

  return (
    <div>
      <MainHeader />
      <div className="flex w-full justify-center pt-20 items-center">
        <Card className="w-[500px]">
          <Form {...form}>
            <form onSubmit={form.handleSubmit(onSubmit)}>
              <CardHeader>
                <CardTitle>Login</CardTitle>
                <CardDescription>
                  Please login your account first.
                </CardDescription>
              </CardHeader>
              <CardContent>
                <div className="grid w-full items-center gap-4">
                  <div className="flex flex-col space-y-1.5">
                    <FormField
                      control={form.control}
                      name="account"
                      render={({ field }) => (
                        <FormItem>
                          <FormLabel>Account</FormLabel>
                          <FormControl>
                            <Input
                              autoFocus
                              placeholder="Please input your account"
                              {...field}
                            />
                          </FormControl>
                          <FormMessage />
                        </FormItem>
                      )}
                    />
                  </div>
                  <div className="flex flex-col space-y-1.5">
                    <FormField
                      control={form.control}
                      name="password"
                      render={({ field }) => (
                        <FormItem>
                          <FormLabel>Password</FormLabel>
                          <FormControl>
                            <Input
                              type="password"
                              placeholder="Please input your password"
                              {...field}
                            />
                          </FormControl>
                          <FormMessage />
                        </FormItem>
                      )}
                    />
                  </div>
                </div>
              </CardContent>
              <CardFooter className="flex justify-between">
                <Button type="submit" className="w-full">
                  {processing ? "Login..." : "Login"}
                </Button>
              </CardFooter>
            </form>
          </Form>
        </Card>
      </div>
    </div>
  );
}
