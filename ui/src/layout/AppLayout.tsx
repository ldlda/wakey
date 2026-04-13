import { useEffect, useState } from "react";
import { NavLink, Outlet } from "react-router-dom";

import { Button } from "@/components/ui/button";

const navItems = [
  ["/", "Devices"],
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
    <div className="app">
      <header className="topbar">
        <div>
          <h1>Wakey Operator UI</h1>
          <p>Find devices fast and wake them reliably</p>
        </div>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() =>
            setTheme((current) => (current === "dark" ? "light" : "dark"))
          }
        >
          {theme === "dark" ? "Switch to light" : "Switch to dark"}
        </Button>
      </header>

      <nav className="tabs">
        {navItems.map(([to, label]) => (
          <NavLink
            key={to}
            to={to}
            end={to === "/"}
            className={({ isActive }) => `tab ${isActive ? "active" : ""}`}
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
