import type { CodegenConfig } from "@graphql-codegen/cli";

const config: CodegenConfig = {
  schema: "./schema.graphql",
  documents: ["src/**/*.ts"],
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
};

export default config;
