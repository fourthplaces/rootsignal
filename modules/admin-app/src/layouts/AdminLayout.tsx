import { useEffect, useState, type ComponentType } from "react";
import { NavLink, Outlet, useNavigate } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ME } from "@/graphql/queries";
import { LOGOUT } from "@/graphql/mutations";
import {
  LayoutDashboard,
  Radar,
  Database,
  Archive,
  Waypoints,
  LogOut,
  ChevronsLeft,
  ChevronsRight,
  type LucideProps,
} from "lucide-react";

const navItems: { to: string; label: string; icon: ComponentType<LucideProps> }[] = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard },
  { to: "/scout", label: "Scout", icon: Radar },
  { to: "/sources", label: "Sources", icon: Database },
  { to: "/graph", label: "Graph", icon: Waypoints },
  { to: "/archive", label: "Archive", icon: Archive },
];

function useCollapsed() {
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem("sidebar-collapsed") === "true"
  );
  const toggle = () =>
    setCollapsed((prev) => {
      const next = !prev;
      localStorage.setItem("sidebar-collapsed", String(next));
      return next;
    });
  return [collapsed, toggle] as const;
}

export function AdminLayout() {
  const navigate = useNavigate();
  const { data, loading, error } = useQuery(ME);
  const [logout] = useMutation(LOGOUT);

  const [collapsed, toggleCollapsed] = useCollapsed();

  useEffect(() => {
    if (!loading && (!data?.me || error)) {
      navigate("/login", { replace: true });
    }
  }, [data, loading, error, navigate]);

  if (loading) {
    return (
      <div className="flex h-screen items-center justify-center">
        <p className="text-muted-foreground">Loading...</p>
      </div>
    );
  }

  if (!data?.me) return null;

  const handleLogout = async () => {
    await logout();
    navigate("/login", { replace: true });
  };

  return (
    <div className="flex h-screen">
      <aside
        className={`${collapsed ? "w-12" : "w-56"} shrink-0 border-r border-border bg-card flex flex-col transition-[width] duration-200`}
      >
        <div className="p-4 border-b border-border overflow-hidden">
          <h1 className="text-sm font-semibold tracking-tight whitespace-nowrap">
            {collapsed ? "RS" : "Root Signal"}
          </h1>
          {!collapsed && (
            <p className="text-xs text-muted-foreground">Admin</p>
          )}
        </div>
        <nav className="flex-1 p-2 space-y-0.5">
          {navItems.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.to === "/"}
              title={collapsed ? item.label : undefined}
              className={({ isActive }) =>
                `flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-colors overflow-hidden whitespace-nowrap ${
                  isActive
                    ? "bg-accent text-accent-foreground"
                    : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
                }`
              }
            >
              <item.icon size={16} className="shrink-0" />
              {!collapsed && item.label}
            </NavLink>
          ))}
        </nav>
        <div className="p-2 border-t border-border space-y-0.5">
          <button
            onClick={handleLogout}
            title={collapsed ? "Logout" : undefined}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-accent/50 rounded-md text-left transition-colors"
          >
            <LogOut size={16} className="shrink-0" />
            {!collapsed && "Logout"}
          </button>
          <button
            onClick={toggleCollapsed}
            title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
            className="w-full flex items-center gap-2 px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-accent/50 rounded-md text-left transition-colors"
          >
            {collapsed ? <ChevronsRight size={16} className="shrink-0" /> : <ChevronsLeft size={16} className="shrink-0" />}
            {!collapsed && "Collapse"}
          </button>
        </div>
      </aside>
      <main className="flex-1 overflow-auto p-6">
        <Outlet />
      </main>
    </div>
  );
}
