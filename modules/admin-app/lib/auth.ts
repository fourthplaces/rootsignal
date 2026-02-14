import { cookies } from "next/headers";
import { redirect } from "next/navigation";

export async function getAuthToken(): Promise<string | undefined> {
  const cookieStore = await cookies();
  return cookieStore.get("auth_token")?.value;
}

export async function requireAuth(): Promise<string> {
  const token = await getAuthToken();
  if (!token) {
    redirect("/login");
  }
  return token;
}
