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
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import router from "@/router";

export default function Login() {
  return (
    <div>
      <MainHeader />
      <div className="flex w-full justify-center pt-20 items-center">
        <Card className="w-[500px]">
          <CardHeader>
            <CardTitle>Login</CardTitle>
            <CardDescription>Please login your account first.</CardDescription>
          </CardHeader>
          <CardContent>
            <form>
              <div className="grid w-full items-center gap-4">
                <div className="flex flex-col space-y-1.5">
                  <Label htmlFor="name">Account</Label>
                  <Input
                    id="account"
                    autoFocus
                    placeholder="Please input your account"
                  />
                </div>
                <div className="flex flex-col space-y-1.5">
                  <Label htmlFor="name">Password</Label>
                  <Input
                    type="password"
                    id="password"
                    placeholder="Please input your password"
                  />
                </div>
              </div>
            </form>
          </CardContent>
          <CardFooter className="flex justify-between">
            <Button
              variant="outline"
              onClick={() => {
                router.navigate(-1);
              }}
            >
              Cancel
            </Button>
            <Button>Login</Button>
          </CardFooter>
        </Card>
      </div>
    </div>
  );
}
