import { Button, } from "@/components/ui/button"
import { Link } from "react-router-dom";
import { cn } from "@/lib/utils"



 
export default function Home() {
  return (
    <div>
      <Button>Click me</Button>
      <Link
          to={"/test"}
        >
          abc
        </Link>
    </div>
  )
}