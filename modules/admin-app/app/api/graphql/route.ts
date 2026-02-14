import { cookies } from "next/headers";
import { NextRequest, NextResponse } from "next/server";

const GRAPHQL_URL = process.env.GRAPHQL_URL || "http://localhost:9081/graphql";

export async function POST(request: NextRequest) {
  const cookieStore = await cookies();
  const token = cookieStore.get("auth_token")?.value;

  const body = await request.text();

  const res = await fetch(GRAPHQL_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Cookie: `auth_token=${token}` } : {}),
    },
    body,
  });

  const data = await res.json();
  return NextResponse.json(data);
}
