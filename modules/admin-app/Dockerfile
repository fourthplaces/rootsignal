FROM node:22-alpine AS base

RUN corepack enable && corepack prepare pnpm@latest --activate

# Install dependencies
FROM base AS deps
WORKDIR /app
COPY pnpm-lock.yaml pnpm-workspace.yaml ./
COPY modules/admin-app/package.json modules/admin-app/
COPY modules/api-client-js/package.json modules/api-client-js/
RUN pnpm install --frozen-lockfile

# Build api-client first, then admin-app
FROM base AS builder
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY --from=deps /app/modules/admin-app/node_modules ./modules/admin-app/node_modules
COPY --from=deps /app/modules/api-client-js/node_modules ./modules/api-client-js/node_modules
COPY pnpm-workspace.yaml ./
COPY modules/api-client-js modules/api-client-js
COPY modules/admin-app modules/admin-app

RUN pnpm --filter @rootsignal/api-client build 2>/dev/null || true
RUN pnpm --filter @rootsignal/admin-app build

# Production
FROM base AS runner
WORKDIR /app
ENV NODE_ENV=production
ENV PORT=3000

RUN addgroup --system --gid 1001 nodejs && \
    adduser --system --uid 1001 nextjs

COPY --from=builder /app/modules/admin-app/public ./public
COPY --from=builder --chown=nextjs:nodejs /app/modules/admin-app/.next/standalone ./
COPY --from=builder --chown=nextjs:nodejs /app/modules/admin-app/.next/static ./modules/admin-app/.next/static

USER nextjs
EXPOSE 3000
CMD ["node", "modules/admin-app/server.js"]
