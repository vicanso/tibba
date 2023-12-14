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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Button } from "@/components/ui/button";
import { zodResolver } from "@hookform/resolvers/zod";
import { useForm } from "react-hook-form";
import * as z from "zod";
import { useState } from "react";
import useUserStore from "@/state/user";
import { goBack } from "@/router";
import { useToast } from "@/components/ui/use-toast";
import { formatError } from "@/helpers/util";

const loginSchema = z.object({
  account: z.string().min(2).max(50),
  password: z.string().min(6).max(50),
});

const registerSchema = z.object({
  account: z.string().min(2).max(50),
  password: z.string().min(6).max(50),
  passwordConfirm: z.string().min(6).max(50),
});

export default function Login() {
  const { toast } = useToast();

  const loginTab = "login";
  const registerTab = "register";

  const [processing, setProcessing] = useState<boolean>(false);
  const [currentTab, setCurrentTab] = useState(loginTab);
  const [login, register, fetch] = useUserStore((state) => [
    state.login,
    state.register,
    state.fetch,
  ]);

  const loginForm = useForm<z.infer<typeof loginSchema>>({
    resolver: zodResolver(loginSchema),
    defaultValues: {
      account: "",
      password: "",
    },
  });
  const registerForm = useForm<z.infer<typeof registerSchema>>({
    resolver: zodResolver(registerSchema),
    defaultValues: {
      account: "",
      password: "",
      passwordConfirm: "",
    },
  });
  async function onLogin(values: z.infer<typeof loginSchema>) {
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
      toast({
        title: "登录失败",
        description: formatError(err),
      });
      console.error(err);
    } finally {
      setProcessing(false);
    }
  }

  async function onRegister(values: z.infer<typeof registerSchema>) {
    if (processing) {
      return;
    }
    setProcessing(true);
    try {
      if (values.password !== values.passwordConfirm) {
        throw new Error("两次输入的密码不一致");
      }
      await register(values.account, values.password);
      setCurrentTab(loginTab);
      toast({
        title: "注册成功",
        description: "你的账号已成功注册，请登录后使用系统功能",
      });
    } catch (err) {
      toast({
        title: "注册失败",
        description: formatError(err),
      });
      console.error(err);
    } finally {
      setProcessing(false);
    }
  }

  const loginCard = (
    <Card>
      <Form {...loginForm}>
        <form onSubmit={loginForm.handleSubmit(onLogin)}>
          <CardHeader>
            <CardTitle>登录</CardTitle>
            <CardDescription>请先登录后才可使用系统功能</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="grid w-full items-center gap-4">
              <div className="flex flex-col space-y-1.5">
                <FormField
                  control={loginForm.control}
                  name="account"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>账号</FormLabel>
                      <FormControl>
                        <Input
                          autoFocus
                          placeholder="请输入你的账号"
                          type="search"
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
                  control={loginForm.control}
                  name="password"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>密码</FormLabel>
                      <FormControl>
                        <Input
                          type="password"
                          placeholder="请输入你的密码"
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
              {processing ? "登录中..." : "登录"}
            </Button>
          </CardFooter>
        </form>
      </Form>
    </Card>
  );

  const registerCard = (
    <Card>
      <Form {...registerForm}>
        <form onSubmit={registerForm.handleSubmit(onRegister)}>
          <CardHeader>
            <CardTitle>注册</CardTitle>
            <CardDescription>请先注册账号后再登录使用系统功能</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="grid w-full items-center gap-4">
              <div className="flex flex-col space-y-1.5">
                <FormField
                  control={registerForm.control}
                  name="account"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>账号</FormLabel>
                      <FormControl>
                        <Input
                          autoFocus
                          placeholder="请输入你的账号"
                          type="search"
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
                  control={registerForm.control}
                  name="password"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>密码</FormLabel>
                      <FormControl>
                        <Input
                          type="password"
                          placeholder="请输入你的密码"
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
                  control={registerForm.control}
                  name="passwordConfirm"
                  render={({ field }) => (
                    <FormItem>
                      <FormLabel>确认密码</FormLabel>
                      <FormControl>
                        <Input
                          type="password"
                          placeholder="请再次输入你的密码"
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
              {processing ? "注册中..." : "注册"}
            </Button>
          </CardFooter>
        </form>
      </Form>
    </Card>
  );

  return (
    <div>
      <MainHeader />
      <div className="flex w-full justify-center pt-20 items-center">
        <Tabs
          value={currentTab}
          className="w-[500px]"
          onValueChange={(value) => {
            setCurrentTab(value);
          }}
        >
          <TabsList className="grid w-full grid-cols-2 mb-5">
            <TabsTrigger value={loginTab}>登录</TabsTrigger>
            <TabsTrigger value={registerTab}>注册</TabsTrigger>
          </TabsList>
          <TabsContent value={loginTab}>{loginCard}</TabsContent>
          <TabsContent value={registerTab}>{registerCard}</TabsContent>
        </Tabs>
      </div>
    </div>
  );
}
