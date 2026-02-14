type GraphQLError = { message: string; path?: string[]; extensions?: Record<string, unknown> };
type GraphQLResponse<T> = { data?: T; errors?: GraphQLError[] };

export class GraphQLClientError extends Error {
  constructor(
    public errors: GraphQLError[],
  ) {
    super(errors[0]?.message ?? "Unknown GraphQL error");
    this.name = "GraphQLClientError";
  }
}

export interface ClientOptions {
  url: string;
  headers?: Record<string, string>;
  locale?: string;
}

export function createClient(options: ClientOptions) {
  const { url, headers = {}, locale } = options;

  const defaultHeaders: Record<string, string> = {
    "Content-Type": "application/json",
    ...headers,
  };

  if (locale) {
    defaultHeaders["Accept-Language"] = locale;
  }

  return {
    async query<T>(
      document: string,
      variables?: Record<string, unknown>,
    ): Promise<T> {
      const res = await fetch(url, {
        method: "POST",
        headers: defaultHeaders,
        body: JSON.stringify({ query: document, variables }),
      });

      if (!res.ok) {
        throw new Error(`HTTP ${res.status}: ${res.statusText}`);
      }

      const json: GraphQLResponse<T> = await res.json();
      if (json.errors?.length) {
        throw new GraphQLClientError(json.errors);
      }
      return json.data!;
    },

    async mutate<T>(
      document: string,
      variables?: Record<string, unknown>,
      authToken?: string,
    ): Promise<T> {
      const mutationHeaders = { ...defaultHeaders };
      if (authToken) {
        mutationHeaders["Cookie"] = `auth_token=${authToken}`;
      }

      const res = await fetch(url, {
        method: "POST",
        headers: mutationHeaders,
        body: JSON.stringify({ query: document, variables }),
      });

      if (!res.ok) {
        throw new Error(`HTTP ${res.status}: ${res.statusText}`);
      }

      const json: GraphQLResponse<T> = await res.json();
      if (json.errors?.length) {
        throw new GraphQLClientError(json.errors);
      }
      return json.data!;
    },
  };
}
