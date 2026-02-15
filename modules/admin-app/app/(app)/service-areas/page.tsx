import { headers } from "next/headers";
import { authedClient } from "@/lib/client";
import { revalidatePath } from "next/cache";

interface ServiceArea {
  id: string;
  city: string;
  state: string;
  isActive: boolean;
  createdAt: string;
}

async function addServiceArea(formData: FormData) {
  "use server";
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);
  const city = formData.get("city") as string;
  const state = formData.get("state") as string;
  if (!city || !state) return;

  await api.mutate(
    `mutation CreateServiceArea($input: CreateServiceAreaInput!) {
      createServiceArea(input: $input) { id }
    }`,
    { input: { city, state } },
  );
  revalidatePath("/service-areas");
}

async function deleteServiceArea(formData: FormData) {
  "use server";
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);
  const id = formData.get("id") as string;

  await api.mutate(
    `mutation DeleteServiceArea($id: UUID!) {
      deleteServiceArea(id: $id)
    }`,
    { id },
  );
  revalidatePath("/service-areas");
}

export default async function ServiceAreasPage() {
  const headerStore = await headers();
  const api = authedClient(headerStore.get("cookie") ?? undefined);

  const { serviceAreas } = await api.query<{ serviceAreas: ServiceArea[] }>(
    `query ServiceAreas {
      serviceAreas { id city state isActive createdAt }
    }`,
  );

  return (
    <div>
      <h1 className="mb-6 text-2xl font-bold">Service Areas</h1>

      <form action={addServiceArea} className="mb-6 flex items-end gap-3">
        <div>
          <label className="block text-xs font-medium text-gray-600 mb-1">City</label>
          <input
            name="city"
            type="text"
            required
            placeholder="Minneapolis"
            className="rounded border border-gray-300 px-3 py-2 text-sm"
          />
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-600 mb-1">State</label>
          <input
            name="state"
            type="text"
            required
            placeholder="MN"
            className="rounded border border-gray-300 px-3 py-2 text-sm w-20"
          />
        </div>
        <button
          type="submit"
          className="rounded bg-green-700 px-4 py-2 text-sm text-white hover:bg-green-800"
        >
          Add
        </button>
      </form>

      <div className="overflow-hidden rounded-lg border border-gray-200 bg-white">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">City</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">State</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Active</th>
              <th className="px-4 py-3 text-left text-xs font-medium uppercase text-gray-500">Added</th>
              <th className="px-4 py-3"></th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-200">
            {serviceAreas.map((sa) => (
              <tr key={sa.id} className="hover:bg-gray-50">
                <td className="px-4 py-3 text-sm font-medium">{sa.city}</td>
                <td className="px-4 py-3 text-sm text-gray-600">{sa.state}</td>
                <td className="px-4 py-3">
                  {sa.isActive ? (
                    <span className="text-green-600">Yes</span>
                  ) : (
                    <span className="text-gray-400">No</span>
                  )}
                </td>
                <td className="px-4 py-3 text-sm text-gray-500">
                  {new Date(sa.createdAt).toLocaleDateString()}
                </td>
                <td className="px-4 py-3 text-right">
                  <form action={deleteServiceArea} className="inline">
                    <input type="hidden" name="id" value={sa.id} />
                    <button
                      type="submit"
                      className="text-sm text-red-600 hover:text-red-800"
                    >
                      Delete
                    </button>
                  </form>
                </td>
              </tr>
            ))}
            {serviceAreas.length === 0 && (
              <tr>
                <td colSpan={5} className="px-4 py-8 text-center text-sm text-gray-500">
                  No service areas yet. Add one above.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
