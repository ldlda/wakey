import { useEffect, useState } from "react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import { useTheme } from "next-themes";
import {
  Moon,
  Sun,
  Monitor,
  Zap,
  BarChart3,
  Bell,
  Bot,
  FileText,
  Key,
  Eye,
  Terminal,
  PanelLeftClose,
  PanelLeft,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

const SIDEBAR_KEY = "wakey-sidebar-collapsed";

type NavSection = {
  label: string;
  items: readonly {
    to: string;
    label: string;
    icon: React.ElementType;
    end?: boolean;
  }[];
};

const navSections: NavSection[] = [
  {
    label: "Operations",
    items: [
      { to: "/", label: "Devices", icon: Monitor, end: true },
      { to: "/wake", label: "Wake Tools", icon: Zap },
    ],
  },
  {
    label: "Monitoring",
    items: [
      { to: "/dashboard", label: "Fleet Health", icon: BarChart3 },
      { to: "/alerts", label: "Alerts", icon: Bell },
      { to: "/observations", label: "Observations", icon: Eye },
    ],
  },
  {
    label: "Admin",
    items: [
      { to: "/agents", label: "Agents", icon: Bot },
      { to: "/audit", label: "Audit", icon: FileText },
      { to: "/tokens", label: "Tokens", icon: Key },
      { to: "/commands", label: "Commands", icon: Terminal },
    ],
  },
];

function usePageTitle() {
  const { pathname } = useLocation();
  useEffect(() => {
    const flat = navSections.flatMap((s) => s.items);
    const match = flat.find((item) =>
      item.end
        ? pathname === "/ui" || pathname === "/ui/" || pathname === "/"
        : pathname.startsWith(item.to) && item.to !== "/",
    );
    document.title = match ? `${match.label} — Wakey` : "Wakey Operator UI";
  }, [pathname]);
}

export function AppLayout() {
  const { resolvedTheme, setTheme } = useTheme();
  const isDark = resolvedTheme === "dark";
  const [collapsed, setCollapsed] = useState(() => {
    return window.localStorage.getItem(SIDEBAR_KEY) === "true";
  });

  usePageTitle();

  useEffect(() => {
    window.localStorage.setItem(SIDEBAR_KEY, String(collapsed));
  }, [collapsed]);

  return (
    <div className="app-shell">
      <aside
        className={`sidebar ${collapsed ? "sidebar--collapsed" : ""}`}
        data-collapsed={collapsed}
      >
        <div className="sidebar-brand">
          <div className="sidebar-logo">
            <Zap className="size-5 text-primary" />
          </div>
          {!collapsed && (
            <div className="sidebar-brand-text">
              <span className="text-sm font-semibold tracking-tight">
                Wakey
              </span>
              <span className="text-[0.65rem] leading-none text-muted-foreground">
                Operator UI
              </span>
            </div>
          )}
        </div>

        <nav className="sidebar-nav">
          {navSections.map((section) => (
            <div key={section.label} className="sidebar-section">
              {!collapsed && (
                <div className="sidebar-section-label">{section.label}</div>
              )}
              {section.items.map((item) => {
                const Icon = item.icon;
                const link = (
                  <NavLink
                    key={item.to}
                    to={item.to}
                    end={item.end}
                    className={({ isActive }) =>
                      [
                        "sidebar-link",
                        isActive ? "sidebar-link--active" : "",
                        collapsed ? "sidebar-link--collapsed" : "",
                      ]
                        .filter(Boolean)
                        .join(" ")
                    }
                  >
                    <Icon className="sidebar-link-icon" />
                    {!collapsed && (
                      <span className="sidebar-link-label">{item.label}</span>
                    )}
                  </NavLink>
                );

                if (collapsed) {
                  return (
                    <Tooltip key={item.to}>
                      <TooltipTrigger>{link}</TooltipTrigger>
                      <TooltipContent side="right">{item.label}</TooltipContent>
                    </Tooltip>
                  );
                }
                return link;
              })}
            </div>
          ))}
        </nav>

        <div className="sidebar-footer">
          <Tooltip>
            <TooltipTrigger>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="sidebar-footer-btn"
                aria-label={isDark ? "Switch to light" : "Switch to dark"}
                onClick={() => setTheme(isDark ? "light" : "dark")}
              >
                {isDark ? (
                  <Sun className="size-4" aria-hidden />
                ) : (
                  <Moon className="size-4" aria-hidden />
                )}
                {!collapsed && (
                  <span className="sidebar-link-label">
                    {isDark ? "Light mode" : "Dark mode"}
                  </span>
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">
              {isDark ? "Switch to light" : "Switch to dark"}
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="sidebar-footer-btn"
                aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
                onClick={() => setCollapsed((c) => !c)}
              >
                {collapsed ? (
                  <PanelLeft className="size-4" aria-hidden />
                ) : (
                  <PanelLeftClose className="size-4" aria-hidden />
                )}
                {!collapsed && (
                  <span className="sidebar-link-label">Collapse</span>
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">
              {collapsed ? "Expand" : "Collapse"}
            </TooltipContent>
          </Tooltip>
        </div>
      </aside>

      <main className="app-main">
        <Outlet />
      </main>
    </div>
  );
}
