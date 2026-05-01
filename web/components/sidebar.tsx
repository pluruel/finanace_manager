"use client";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { cn } from "@/lib/utils";
import {
  LayoutDashboard,
  ListOrdered,
  Upload,
  Tag,
  TrendingUp,
  LogOut,
} from "lucide-react";
import { Button } from "@/components/ui/button";

type NavItem = {
  href: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  disabled?: boolean;
};

const navItems: NavItem[] = [
  {
    href: "/",
    label: "대시보드",
    icon: LayoutDashboard,
  },
  {
    href: "/transactions",
    label: "거래 내역",
    icon: ListOrdered,
  },
  {
    href: "/import",
    label: "임포트",
    icon: Upload,
  },
  {
    href: "/aliases",
    label: "정규화",
    icon: Tag,
    disabled: true,
  },
  {
    href: "/price-history",
    label: "가격 추적",
    icon: TrendingUp,
    disabled: true,
  },
];

export function Sidebar() {
  const pathname = usePathname();
  const router = useRouter();

  async function handleLogout() {
    await fetch("/api/auth/logout", { method: "POST" });
    router.push("/login");
  }

  return (
    <aside className="flex flex-col w-56 min-h-screen border-r bg-background shrink-0">
      <div className="p-4 border-b">
        <h1 className="font-bold text-lg leading-tight">가계부 뷰어</h1>
      </div>
      <nav className="flex-1 p-3 space-y-1" aria-label="메인 네비게이션">
        {navItems.map((item) => {
          const isActive = pathname === item.href;
          const Icon = item.icon;

          if (item.disabled) {
            return (
              <div
                key={item.href}
                className="flex items-center gap-3 px-3 py-2 rounded-md text-sm text-muted-foreground cursor-not-allowed opacity-50"
                title="준비 중"
                aria-disabled="true"
              >
                <Icon className="h-4 w-4" />
                <span>{item.label}</span>
                <span className="ml-auto text-xs bg-muted rounded px-1.5 py-0.5">
                  준비중
                </span>
              </div>
            );
          }

          return (
            <Link
              key={item.href}
              href={item.href}
              className={cn(
                "flex items-center gap-3 px-3 py-2 rounded-md text-sm transition-colors",
                isActive
                  ? "bg-primary text-primary-foreground"
                  : "hover:bg-accent hover:text-accent-foreground",
              )}
              aria-current={isActive ? "page" : undefined}
            >
              <Icon className="h-4 w-4" />
              <span>{item.label}</span>
            </Link>
          );
        })}
      </nav>
      <div className="p-3 border-t">
        <Button
          variant="ghost"
          className="w-full justify-start gap-3 text-muted-foreground hover:text-foreground"
          onClick={handleLogout}
        >
          <LogOut className="h-4 w-4" />
          <span>로그아웃</span>
        </Button>
      </div>
    </aside>
  );
}
