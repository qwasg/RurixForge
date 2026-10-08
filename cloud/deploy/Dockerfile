# forge-cloud 发布镜像：管理后台（Vite）→ Go 内嵌（-tags embedui）→ 精简运行镜像。
# 构建（仓库根）：docker build -f cloud/deploy/Dockerfile -t rurixforge/forge-cloud cloud
# 构建上下文是 cloud/ 目录（后台依赖在其中独立安装，不走 monorepo 锁文件）。

FROM node:22-alpine AS web
WORKDIR /web
RUN corepack enable && corepack prepare pnpm@11.5.0 --activate
COPY web/package.json ./
# 与仓库 packageManager 一致；只执行 Vite 所需的 esbuild 安装脚本。
RUN pnpm install --ignore-workspace --no-frozen-lockfile --ignore-scripts \
    && node -e "require('node:module').createRequire(require.resolve('vite/package.json'))('esbuild/install.js')"
COPY web/ ./
RUN pnpm run build

FROM golang:1.26-alpine AS build
ARG VERSION=0.1.0
WORKDIR /src
COPY go.mod go.sum ./
RUN go mod download
COPY . .
COPY --from=web /web/dist ./internal/webui/dist
RUN CGO_ENABLED=0 go build -tags embedui -trimpath \
    -ldflags "-s -w -X forge-cloud/internal/config.Version=${VERSION}" \
    -o /out/forge-cloud ./cmd/forge-cloud

FROM alpine:3.20
RUN apk add --no-cache ca-certificates tzdata \
    && adduser -D -u 10001 forge
COPY --from=build /out/forge-cloud /usr/local/bin/forge-cloud
USER forge
ENV FORGE_CLOUD_ADDR=:8110
EXPOSE 8110
HEALTHCHECK --interval=15s --timeout=3s --retries=5 CMD wget -qO- http://127.0.0.1:8110/healthz >/dev/null || exit 1
ENTRYPOINT ["forge-cloud"]
CMD ["serve"]
