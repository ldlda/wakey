import { useEffect, useState } from "react";
import { NavLink, Outlet } from "react-router-dom";
import { ChevronDown, Moon, Sun } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

const navItems = [
  ["/", "Devices"],
  ["/dashboard", "Fleet Health"],
] as const;

const adminItems = [
  ["/agents", "Agents"],
  ["/tokens", "Tokens"],
  ["/alerts", "Alerts"],
  ["/observations", "Raw Observations"],
  ["/commands", "Command Runner"],
  ["/audit", "Audit"],
] as const;

type Theme = "light" | "dark";

const THEME_KEY = "wakey-ui-theme";

function resolveInitialTheme(): Theme {
  const stored = window.localStorage.getItem(THEME_KEY);
  if (stored === "light" || stored === "dark") return stored;

  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export function AppLayout() {
  const [theme, setTheme] = useState<Theme>(() => resolveInitialTheme());

  useEffect(() => {
    const root = document.documentElement;
    root.classList.toggle("dark", theme === "dark");
    window.localStorage.setItem(THEME_KEY, theme);
  }, [theme]);

  return (
    <div className="mx-auto max-w-7xl p-4">
      <header className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">
            Wakey
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">
            Fleet devices, presence, and wake controls
          </p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="fixed right-3 top-3 z-50 size-9 rounded-full p-0 sm:static sm:size-auto sm:rounded-md sm:px-3 sm:py-2"
          aria-label={theme === "dark" ? "Switch to light" : "Switch to dark"}
          title={theme === "dark" ? "Switch to light" : "Switch to dark"}
          onClick={() =>
            setTheme((current) => (current === "dark" ? "light" : "dark"))
          }
        >
          {theme === "dark" ? (
            <Sun className="size-4" aria-hidden />
          ) : (
            <Moon className="size-4" aria-hidden />
          )}
          <span className="hidden sm:inline">
            {theme === "dark" ? "Switch to light" : "Switch to dark"}
          </span>
        </Button>
      </header>

      <nav className="mb-3 flex flex-wrap items-center gap-2">
        {navItems.map(([to, label]) => (
          <NavLink
            key={to}
            to={to}
            end={to === "/"}
            className={({ isActive }) =>
              [
                "rounded-md border px-3 py-1.5 text-sm transition-colors",
                isActive
                  ? "border-primary/60 bg-primary/10 text-foreground"
                  : "border-border bg-card text-muted-foreground hover:text-foreground",
              ].join(" ")
            }
          >
            {label}
          </NavLink>
        ))}
        <DropdownMenu>
          <DropdownMenuTrigger className="inline-flex items-center gap-1 rounded-md border border-border bg-card px-3 py-1.5 text-sm text-muted-foreground hover:text-foreground">
            Admin / Debug
            <ChevronDown className="size-4" aria-hidden />
          </DropdownMenuTrigger>
          <DropdownMenuContent className="w-48">
            {adminItems.map(([to, label]) => (
              <DropdownMenuItem key={to}>
                <NavLink className="w-full" to={to}>
                  {label}
                </NavLink>
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </nav>

      <main>
        <Outlet />
      </main>
    </div>
  );
}
