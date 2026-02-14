import type { CodegenConfig } from "@graphql-codegen/cli";

const config: CodegenConfig = {
  schema: "./schema.graphql",
  generates: {
    "./gql/": {
      preset: "client",
      config: {
        fragmentMasking: false,
        scalars: {
          DateTime: "string",
          UUID: "string",
          JSON: "Record<string, unknown>",
        },
      },
    },
  },
  ignoreNoDocuments: true,
};

export default config;
