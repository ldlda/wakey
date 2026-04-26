import { useEffect, useState } from "react";
import { NavLink, Outlet } from "react-router-dom";
import { Moon, Sun } from "lucide-react";

import { Button } from "@/components/ui/button";

const navItems = [
  ["/", "Devices"],
  ["/observations", "Observed"],
  ["/commands", "Wake Tools"],
  ["/dashboard", "Fleet Health"],
  ["/agents", "Agents"],
  ["/audit", "Audit"],
  ["/alerts", "Alerts"],
  ["/tokens", "Tokens"],
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
            Wakey Operator UI
          </h1>
          <p className="mt-1 text-sm text-muted-foreground">
            Find devices fast and wake them reliably
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

      <nav className="mb-3 flex flex-wrap gap-2">
        {navItems.map(([to, label]) => (
          <NavLink
            key={to}
            to={to}
            end={to === "/"}
            className={({ isActive }) =>
              [
                "rounded-full border px-3 py-1.5 text-sm transition-colors",
                isActive
                  ? "border-primary/60 bg-primary/10 text-foreground"
                  : "border-border bg-card text-muted-foreground hover:text-foreground",
              ].join(" ")
            }
          >
            {label}
          </NavLink>
        ))}
      </nav>

      <main>
        <Outlet />
      </main>
    </div>
  );
}
