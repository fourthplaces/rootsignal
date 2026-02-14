import { createClient } from "@rootsignal/api-client";

const GRAPHQL_URL = process.env.GRAPHQL_URL || "http://localhost:9081/graphql";

export const client = createClient({ url: GRAPHQL_URL });

export function authedClient(cookieHeader?: string) {
  return createClient({
    url: GRAPHQL_URL,
    headers: cookieHeader ? { Cookie: cookieHeader } : undefined,
  });
}
