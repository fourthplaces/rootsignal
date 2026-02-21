import { useEffect } from "react";
import { NavLink, Outlet, useNavigate } from "react-router";
import { useQuery, useMutation } from "@apollo/client";
import { ME } from "@/graphql/queries";
import { LOGOUT } from "@/graphql/mutations";

const navItems = [
  { to: "/", label: "Dashboard" },
  { to: "/scout", label: "Scout" },
  { to: "/signals", label: "Signals" },
  { to: "/stories", label: "Stories" },
  { to: "/actors", label: "Actors" },
  { to: "/findings", label: "Findings" },
];

export function AdminLayout() {
  const navigate = useNavigate();
  const { data, loading, error } = useQuery(ME);
  const [logout] = useMutation(LOGOUT);

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
      <aside className="w-56 shrink-0 border-r border-border bg-card flex flex-col">
        <div className="p-4 border-b border-border">
          <h1 className="text-sm font-semibold tracking-tight">Root Signal</h1>
          <p className="text-xs text-muted-foreground">Admin</p>
        </div>
        <nav className="flex-1 p-2 space-y-0.5">
          {navItems.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.to === "/"}
              className={({ isActive }) =>
                `block px-3 py-2 rounded-md text-sm transition-colors ${
                  isActive
                    ? "bg-accent text-accent-foreground"
                    : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
                }`
              }
            >
              {item.label}
            </NavLink>
          ))}
        </nav>
        <div className="p-2 border-t border-border">
          <button
            onClick={handleLogout}
            className="w-full px-3 py-2 text-sm text-muted-foreground hover:text-foreground hover:bg-accent/50 rounded-md text-left transition-colors"
          >
            Logout
          </button>
        </div>
      </aside>
      <main className="flex-1 overflow-auto p-6">
        <Outlet />
      </main>
    </div>
  );
}
