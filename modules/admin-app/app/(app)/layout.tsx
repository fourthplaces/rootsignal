import { cookies } from "next/headers";
import { redirect } from "next/navigation";
import Link from "next/link";
import { logout } from "@/lib/actions";

const NAV_ITEMS = [
  { href: "/", label: "Dashboard" },
  { href: "/listings", label: "Listings" },
  { href: "/entities", label: "Entities" },
  { href: "/sources", label: "Sources" },
  { href: "/service-areas", label: "Service Areas" },
  { href: "/tags", label: "Tags" },
  { href: "/observations", label: "Observations" },
  { href: "/stats", label: "Stats" },
  { href: "/map", label: "Map" },
];

export default async function AppLayout({
  children,
  modal,
}: {
  children: React.ReactNode;
  modal: React.ReactNode;
}) {
  const cookieStore = await cookies();
  const token = cookieStore.get("auth_token")?.value;
  if (!token) {
    redirect("/login");
  }

  return (
    <div className="flex min-h-screen">
      <aside className="w-56 shrink-0 border-r border-gray-200 bg-white">
        <div className="p-4">
          <h2 className="text-lg font-bold text-green-800">Root Signal</h2>
        </div>
        <nav className="space-y-1 px-2">
          {NAV_ITEMS.map((item) => (
            <Link
              key={item.href}
              href={item.href}
              className="block rounded px-3 py-2 text-sm text-gray-700 hover:bg-gray-100"
            >
              {item.label}
            </Link>
          ))}
        </nav>
        <div className="mt-auto border-t border-gray-200 p-4">
          <form action={logout}>
            <button
              type="submit"
              className="text-sm text-gray-500 hover:text-gray-700"
            >
              Sign out
            </button>
          </form>
        </div>
      </aside>
      <main className="flex-1 p-6">{children}</main>
      {modal}
    </div>
  );
}
