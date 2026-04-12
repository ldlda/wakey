import { NavLink, Outlet } from "react-router-dom";

const navItems = [
  ["/", "Devices"],
  ["/commands", "Wake Tools"],
  ["/dashboard", "Fleet Health"],
  ["/agents", "Agents"],
  ["/audit", "Audit"],
  ["/alerts", "Alerts"],
  ["/tokens", "Tokens"],
] as const;

export function AppLayout() {
  return (
    <div className="app">
      <header className="topbar">
        <div>
          <h1>Wakey Operator UI</h1>
          <p>Find devices fast and wake them reliably</p>
        </div>
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
