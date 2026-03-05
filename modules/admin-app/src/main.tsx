import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ApolloProvider } from "@apollo/client";
import { BrowserRouter } from "react-router";
import { client } from "@/lib/graphql-client";
import { RegionProvider } from "@/contexts/RegionContext";
import App from "./App";
import "./index.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ApolloProvider client={client}>
      <RegionProvider>
        <BrowserRouter>
          <App />
        </BrowserRouter>
      </RegionProvider>
    </ApolloProvider>
  </StrictMode>,
);
