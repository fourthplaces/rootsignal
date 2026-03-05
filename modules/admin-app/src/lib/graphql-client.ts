import {
  ApolloClient,
  InMemoryCache,
  createHttpLink,
  from,
  split,
} from "@apollo/client";
import { onError } from "@apollo/client/link/error";
import { GraphQLWsLink } from "@apollo/client/link/subscriptions";
import { getMainDefinition } from "@apollo/client/utilities";
import { createClient } from "graphql-ws";

const apiUrl = import.meta.env.VITE_API_URL ?? "";

const httpLink = createHttpLink({
  uri: `${apiUrl}/graphql`,
  credentials: "include",
});

const wsProtocol = window.location.protocol === "https:" ? "wss:" : "ws:";
const wsHost = apiUrl
  ? apiUrl.replace(/^https?:/, wsProtocol)
  : `${wsProtocol}//${window.location.host}`;

const wsLink = new GraphQLWsLink(
  createClient({
    url: `${wsHost}/graphql/ws`,
  }),
);

const errorLink = onError(({ graphQLErrors }) => {
  if (graphQLErrors) {
    for (const err of graphQLErrors) {
      if (err.extensions?.code === "UNAUTHENTICATED") {
        if (window.location.pathname !== "/login") {
          window.location.href = "/login";
        }
        return;
      }
    }
  }
});

// Route subscriptions over WebSocket, everything else over HTTP
const splitLink = split(
  ({ query }) => {
    const def = getMainDefinition(query);
    return def.kind === "OperationDefinition" && def.operation === "subscription";
  },
  wsLink,
  from([errorLink, httpLink]),
);

export const client = new ApolloClient({
  link: splitLink,
  cache: new InMemoryCache(),
  defaultOptions: {
    watchQuery: { fetchPolicy: "cache-and-network" },
  },
});
